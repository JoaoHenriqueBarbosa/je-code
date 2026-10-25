//! Porte de `src/vs/workbench/browser/composite.ts` (e do `Component` de `common/component.ts`, do `Themable` de
//! `platform/theme/common/themeService.ts` e de `IComposite`/`ICompositeControl` de `common/composite.ts`, que ele estende e
//! implementa): o [`Composite`], o [`CompositeDescriptor`] e o [`CompositeRegistry`].
//!
//! Divergências:
//! - **Herança vira composição.** A classe abstrata `Composite` que os painéis estendem é o [`Composite`] concreto, e o que as
//!   subclasses sobrescrevem (`getTitle`, `create`, `setVisible`, `focus`, `layout`, `setBoundarySashes`, `getMenuIds`,
//!   `getActions`, `getSecondaryActions`, `getContextMenuActions`, `getActionViewItem`, `getActionsContext`, `getActionRunner`,
//!   `getControl`, `updateStyles` e `saveState`) é o trait [`CompositeBehavior`]. A chamada a `super.create(parent)`,
//!   `super.setVisible(visible)` e `super.getActionRunner()` de uma sobrescrita é [`Composite::create_base`],
//!   [`Composite::set_visible_base`] e [`Composite::get_action_runner_base`]. Quem estende o composite recebe a `&Composite` nos
//!   ganchos que chamam o `super`, e guarda uma referência fraca a ela se precisa do `updateTitleArea` ou do `getContainer` fora
//!   deles. O `updateTitleArea` (`protected`) é público, porque quem estende é um [`CompositeBehavior`] de fora do módulo.
//! - **`IComposite`.** Os sete membros da interface (`onDidFocus`, `onDidBlur`, `hasFocus`, `getId`, `getTitle`, `getControl` e
//!   `focus`) existem só como o trait [`IComposite`] implementado pelo [`Composite`] (sem método próprio repetido), então quem
//!   chama precisa do trait no escopo. `getId` devolve a `String` do contrato.
//! - **DOM vira estado nativo.** O contêiner é o [`ViewElement`]. O `trackFocus(container)` do `onDidFocus` e do `onDidBlur` é a
//!   [`FocusTrackerFactory`] que quem cria o composite entrega ([`native_focus_tracker_factory`] liga o `FocusTracker` da árvore
//!   de widgets ao nó do contêiner); sem fábrica não há fonte de foco e os dois eventos nunca disparam.
//! - **`Component` e `Themable`.** O `Memento` é o de `common/memento`; o `onWillSaveState` pede o `saveState` do composite e grava
//!   o memento, e o `onDidColorThemeChange` troca o tema e chama o `updateStyles`, como o `Component` e o `Themable`.
//! - **`CompositeDescriptor<T>` e `CompositeRegistry<T>`.** São genéricos no produto (`T`), como o original; `IConstructorSignature`
//!   com `instantiationService.createInstance(ctor)` é a [`CompositeFactory`], que recebe o `InstantiationService`. O que o
//!   `CompositePart` pergunta ao registro (o nome e a fábrica de um identificador) é o [`ICompositeRegistry`], implementado pelo
//!   registro de qualquer produto que seja um [`CompositeInstance`]. `registerComposite`, `deregisterComposite` e `getComposites`
//!   (`protected`) são públicos pelo mesmo motivo do `updateTitleArea`.
//! - **`IBoundarySashes`** é o `BoundarySashes` de `je-widgets`; `IDomPosition` é o [`DomPosition`] de `part`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, ActionRunner, IActionRunner};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_instantiation::InstantiationService;
use je_platform::actions::MenuId;
use je_platform::contextview::view_element::ViewElement;
use je_platform::storage::{StorageScope, StorageService, StorageTarget, StorageValueChangeEvent};
use je_platform::telemetry::ITelemetryService;
use je_theme::color::Color;
use je_ui::Ui;
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::dom::FocusTracker;
use je_widgets::sash::BoundarySashes;

use crate::browser::part::{DomPosition, assert_returns_defined};
use crate::browser::parts::part_support::{ColorModifier, PartTheme};
use crate::common::memento::Memento;
use crate::services::contextview::native_context_view_service::native_node;

/// `ICompositeControl`: a interface marcadora do controle de um composite.
pub trait ICompositeControl {}

/// `IComposite`.
pub trait IComposite {
    /// An event when the composite gained focus.
    fn on_did_focus(&self) -> Event<()>;

    /// An event when the composite lost focus.
    fn on_did_blur(&self) -> Event<()>;

    /// Returns true if the composite has focus.
    fn has_focus(&self) -> bool;

    /// Returns the unique identifier of this composite.
    fn get_id(&self) -> String;

    /// Returns the name of this composite to show in the title area.
    fn get_title(&self) -> Option<String>;

    /// Returns the underlying control of this composite.
    fn get_control(&self) -> Option<Rc<dyn ICompositeControl>>;

    /// Asks the underlying control to focus.
    fn focus(&self);
}

/// O que o `trackFocus(container)` do `dom.ts` entrega: os avisos de foco e de perda de foco do contêiner.
pub trait IFocusTracker: IDisposable {
    /// `onDidFocus`.
    fn on_did_focus(&self) -> Event<()>;

    /// `onDidBlur`.
    fn on_did_blur(&self) -> Event<()>;
}

impl IFocusTracker for FocusTracker {
    fn on_did_focus(&self) -> Event<()> {
        FocusTracker::on_did_focus(self)
    }

    fn on_did_blur(&self) -> Event<()> {
        FocusTracker::on_did_blur(self)
    }
}

/// `trackFocus(container)`.
pub type FocusTrackerFactory = Rc<dyn Fn(&Rc<ViewElement>) -> Rc<dyn IFocusTracker>>;

/// O `trackFocus` sobre a árvore de widgets: o contêiner precisa ser o elemento de um nó do `Ui`.
pub fn native_focus_tracker_factory(ui: Ui) -> FocusTrackerFactory {
    Rc::new(move |container: &Rc<ViewElement>| {
        let node = assert_returns_defined(native_node(container));
        let tracker: Rc<dyn IFocusTracker> = FocusTracker::new(&ui, node);
        tracker
    })
}

/// O que as subclasses de `Composite` sobrescrevem. Os padrões são os do original.
pub trait CompositeBehavior {
    /// `getTitle()`.
    fn get_title(&self) -> Option<String> {
        None
    }

    /// `create(parent)`: o padrão é o `Composite.create`.
    fn create(&self, composite: &Composite, parent: &Rc<ViewElement>) {
        composite.create_base(parent);
    }

    /// `setVisible(visible)`: o padrão é o `Composite.setVisible`.
    fn set_visible(&self, composite: &Composite, visible: bool) {
        composite.set_visible_base(visible);
    }

    /// `focus()`: subclasses can implement.
    fn focus(&self) {}

    /// `layout(dimension, position?)`: layout the contents of this composite using the provided dimensions.
    fn layout(&self, dimension: Dimension, position: Option<DomPosition>);

    /// `setBoundarySashes(sashes)`: set boundary sashes for this composite. These are used to create draggable corner areas with
    /// inner sashes.
    fn set_boundary_sashes(&self, sashes: &BoundarySashes);

    /// `getMenuIds()`.
    fn get_menu_ids(&self) -> Vec<MenuId> {
        Vec::new()
    }

    /// `getActions()`: an array of actions to show in the action bar of the composite.
    fn get_actions(&self) -> Vec<ActionRef> {
        Vec::new()
    }

    /// `getSecondaryActions()`: an array of actions to show in the action bar of the composite in a less prominent way than the
    /// actions from `getActions`.
    fn get_secondary_actions(&self) -> Vec<ActionRef> {
        Vec::new()
    }

    /// `getContextMenuActions()`: an array of actions to show in the context menu of the composite.
    fn get_context_menu_actions(&self) -> Vec<ActionRef> {
        Vec::new()
    }

    /// `getActionViewItem(action, options)`: for any of the actions returned by this composite, provide an `IActionViewItem` in
    /// cases where the implementor of the composite wants to override the presentation of an action. `None` indicates that the
    /// action is not rendered through an action item.
    fn get_action_view_item(&self, _action: &ActionRef, _options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        None
    }

    /// `getActionsContext()`: provide a context to be passed to the toolbar.
    fn get_actions_context(&self) -> Option<Dynamic> {
        None
    }

    /// `getActionRunner()`: o padrão é o `Composite.getActionRunner`.
    fn get_action_runner(&self, composite: &Composite) -> Rc<dyn IActionRunner> {
        composite.get_action_runner_base()
    }

    /// `getControl()`: the underlying composite control or `None` if it is not accessible.
    fn get_control(&self) -> Option<Rc<dyn ICompositeControl>> {
        None
    }

    /// `updateStyles()` (do `Themable`): subclasses to override.
    fn update_styles(&self, _composite: &Composite) {}

    /// `saveState()` (do `Component`): subclasses to implement for storing state.
    fn save_state(&self) {}
}

/// A emissão e a escuta de foco que o `registerFocusTrackEvents` cria uma vez.
struct FocusEvents {
    on_did_focus: Rc<Emitter<()>>,
    on_did_blur: Rc<Emitter<()>>,
}

/// Composites are layed out in the sidebar and panel part of the workbench. At a time only one composite can be open in the
/// sidebar, and only one composite can be open in the panel.
///
/// Each composite has a minimized representation that is good enough to provide some information about the state of the
/// composite data.
///
/// The workbench will keep a composite alive after it has been created and show/hide it based on user interaction. The lifecycle
/// of a composite goes in the order create(), setVisible(true|false), layout(), focus(), dispose(). During use of the workbench,
/// a composite will often receive a setVisible, layout and focus call, but only one create and dispose call.
pub struct Composite {
    id: String,
    telemetry_service: Rc<dyn ITelemetryService>,
    theme: PartTheme,
    memento: Memento,
    base: Rc<Disposable>,
    weak_self: Weak<Composite>,
    behavior: Rc<dyn CompositeBehavior>,
    focus_tracker_factory: Option<FocusTrackerFactory>,
    on_title_area_update: Rc<Emitter<()>>,
    focus_events: RefCell<Option<FocusEvents>>,
    has_focus: Cell<bool>,
    action_runner: RefCell<Option<Rc<dyn IActionRunner>>>,
    visible: Cell<bool>,
    parent: RefCell<Option<Rc<ViewElement>>>,
}

impl Composite {
    /// `new Composite(id, telemetryService, themeService, storageService)`, com o que a subclasse sobrescreve em `behavior` e a
    /// fonte de foco do contêiner em `focus_tracker_factory`.
    pub fn new(
        id: &str,
        telemetry_service: Rc<dyn ITelemetryService>,
        theme_service: Rc<dyn IThemeService>,
        storage_service: Rc<dyn StorageService>,
        focus_tracker_factory: Option<FocusTrackerFactory>,
        behavior: Rc<dyn CompositeBehavior>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_title_area_update: Rc<Emitter<()>> = Emitter::new();
        base.register(on_title_area_update.clone());

        let this = Rc::new_cyclic(|weak| Self {
            id: id.to_owned(),
            telemetry_service,
            theme: PartTheme::new(theme_service.get_color_theme()),
            memento: Memento::new(id, storage_service.clone()),
            base,
            weak_self: weak.clone(),
            behavior,
            focus_tracker_factory,
            on_title_area_update,
            focus_events: RefCell::new(None),
            has_focus: Cell::new(false),
            action_runner: RefCell::new(None),
            visible: Cell::new(false),
            parent: RefCell::new(None),
        });

        // Themable: `this._register(this.themeService.onDidColorThemeChange(theme => this.onThemeChange(theme)))`.
        let weak = Rc::downgrade(&this);
        this.base.register(theme_service.on_did_color_theme_change().listen(move |theme| {
            if let Some(this) = weak.upgrade() {
                this.on_theme_change(theme.clone());
            }
        }));

        // Component: `storageService.onWillSaveState`: pede ao composite o estado e grava o memento.
        let weak = Rc::downgrade(&this);
        this.base.register(storage_service.on_will_save_state().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                // Ask the component to persist state into the memento
                this.save_state();

                // Then save the memento into storage
                this.memento.save_memento();
            }
        }));

        this
    }

    /// `telemetryService`.
    pub fn telemetry_service(&self) -> &Rc<dyn ITelemetryService> {
        &self.telemetry_service
    }

    /// `onTitleAreaUpdate`.
    pub fn on_title_area_update(&self) -> Event<()> {
        self.on_title_area_update.event()
    }

    /// `registerFocusTrackEvents()`: cria os dois emissores uma única vez, ligados ao rastreador de foco do contêiner. `None`
    /// quando não há fábrica de rastreador.
    fn focus_events(&self) -> Option<(Rc<Emitter<()>>, Rc<Emitter<()>>)> {
        if let Some(events) = self.focus_events.borrow().as_ref() {
            return Some((events.on_did_focus.clone(), events.on_did_blur.clone()));
        }
        let factory = self.focus_tracker_factory.clone()?;

        let container = assert_returns_defined(self.get_container());
        let focus_tracker = factory(&container);
        self.base.register(focus_tracker.clone());

        let on_did_focus: Rc<Emitter<()>> = Emitter::new();
        self.base.register(on_did_focus.clone());
        let weak = self.weak_self.clone();
        let emitter = on_did_focus.clone();
        self.base.register(focus_tracker.on_did_focus().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.has_focus.set(true);
            }

            emitter.fire(());
        }));

        let on_did_blur: Rc<Emitter<()>> = Emitter::new();
        self.base.register(on_did_blur.clone());
        let weak = self.weak_self.clone();
        let emitter = on_did_blur.clone();
        self.base.register(focus_tracker.on_did_blur().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.has_focus.set(false);
            }

            emitter.fire(());
        }));

        *self.focus_events.borrow_mut() = Some(FocusEvents { on_did_focus: on_did_focus.clone(), on_did_blur: on_did_blur.clone() });
        Some((on_did_focus, on_did_blur))
    }

    /// Note: Clients should not call this method, the workbench calls this method. Calling it otherwise may result in unexpected
    /// behavior.
    ///
    /// Called to create this composite on the provided parent. This method is only called once during the lifetime of the
    /// workbench. Note that DOM-dependent calculations should be performed from the setVisible() call. Only then the composite
    /// will be part of the DOM.
    pub fn create(&self, parent: &Rc<ViewElement>) {
        self.behavior.create(self, parent);
    }

    /// `Composite.create`: guarda o contêiner (o `super.create` das subclasses).
    pub fn create_base(&self, parent: &Rc<ViewElement>) {
        *self.parent.borrow_mut() = Some(parent.clone());
    }

    /// Returns the container this composite is being build in.
    pub fn get_container(&self) -> Option<Rc<ViewElement>> {
        self.parent.borrow().clone()
    }

    /// Note: Clients should not call this method, the workbench calls this method. Calling it otherwise may result in unexpected
    /// behavior.
    ///
    /// Called to indicate that the composite has become visible or hidden. This method is called more than once during workbench
    /// lifecycle depending on the user interaction. The composite will be on-DOM if visible is set to true and off-DOM otherwise.
    ///
    /// Typically this operation should be fast though because setVisible might be called many times during a session. If there is
    /// a long running operation it is fine to have it running in the background asyncly and return before.
    pub fn set_visible(&self, visible: bool) {
        self.behavior.set_visible(self, visible);
    }

    /// `Composite.setVisible` (o `super.setVisible` das subclasses).
    pub fn set_visible_base(&self, visible: bool) {
        if self.visible.get() != visible {
            self.visible.set(visible);
        }
    }

    /// Layout the contents of this composite using the provided dimensions.
    pub fn layout(&self, dimension: Dimension, position: Option<DomPosition>) {
        self.behavior.layout(dimension, position);
    }

    /// Set boundary sashes for this composite.
    pub fn set_boundary_sashes(&self, sashes: &BoundarySashes) {
        self.behavior.set_boundary_sashes(sashes);
    }

    /// `getMenuIds()`.
    pub fn get_menu_ids(&self) -> Vec<MenuId> {
        self.behavior.get_menu_ids()
    }

    /// Returns an array of actions to show in the action bar of the composite.
    pub fn get_actions(&self) -> Vec<ActionRef> {
        self.behavior.get_actions()
    }

    /// Returns an array of actions to show in the action bar of the composite in a less prominent way than the actions from
    /// `get_actions`.
    pub fn get_secondary_actions(&self) -> Vec<ActionRef> {
        self.behavior.get_secondary_actions()
    }

    /// Returns an array of actions to show in the context menu of the composite.
    pub fn get_context_menu_actions(&self) -> Vec<ActionRef> {
        self.behavior.get_context_menu_actions()
    }

    /// `getActionViewItem(action, options)`.
    pub fn get_action_view_item(&self, action: &ActionRef, options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        self.behavior.get_action_view_item(action, options)
    }

    /// Provide a context to be passed to the toolbar.
    pub fn get_actions_context(&self) -> Option<Dynamic> {
        self.behavior.get_actions_context()
    }

    /// Returns the instance of `IActionRunner` to use with this composite for the composite tool bar.
    pub fn get_action_runner(&self) -> Rc<dyn IActionRunner> {
        self.behavior.get_action_runner(self)
    }

    /// `Composite.getActionRunner`: o `ActionRunner` criado na primeira chamada (o `super.getActionRunner` das subclasses).
    pub fn get_action_runner_base(&self) -> Rc<dyn IActionRunner> {
        if let Some(action_runner) = self.action_runner.borrow().clone() {
            return action_runner;
        }

        let action_runner: Rc<dyn IActionRunner> = ActionRunner::new();
        self.base.register(action_runner.clone());
        *self.action_runner.borrow_mut() = Some(action_runner.clone());

        action_runner
    }

    /// Method for composite implementors to indicate to the composite container that the title or the actions of the composite
    /// have changed. Calling this method will cause the container to ask for title (getTitle()) and actions (getActions(),
    /// getSecondaryActions()) if the composite is visible or the next time the composite gets visible.
    pub fn update_title_area(&self) {
        self.on_title_area_update.fire(());
    }

    /// Returns true if this composite is currently visible and false otherwise.
    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }

    // region: Themable

    /// `this.theme`.
    pub fn theme(&self) -> &PartTheme {
        &self.theme
    }

    /// `getColor(id, modify)`.
    pub fn get_color(&self, id: &str, modify: Option<ColorModifier<'_>>) -> Option<Color> {
        self.theme.get_color(id, modify)
    }

    /// `Themable.onThemeChange`: troca o tema e atualiza os estilos.
    fn on_theme_change(&self, theme: Rc<dyn IColorTheme>) {
        self.theme.set(theme);
        self.update_styles();
    }

    /// `updateStyles()`.
    pub fn update_styles(&self) {
        self.behavior.update_styles(self);
    }

    // endregion

    // region: Component

    /// `getMemento(scope, target)`.
    pub fn get_memento(&self, scope: StorageScope, target: StorageTarget) -> ObjectRef {
        self.memento.get_memento(scope, target)
    }

    /// `reloadMemento(scope)`.
    pub fn reload_memento(&self, scope: StorageScope) {
        self.memento.reload_memento(scope);
    }

    /// `onDidChangeMementoValue(scope, disposables)`.
    pub fn on_did_change_memento_value(&self, scope: StorageScope, disposables: &DisposableStore) -> Event<StorageValueChangeEvent> {
        self.memento.on_did_change_value(scope, disposables)
    }

    /// `saveState()`: as subclasses guardam o estado no memento.
    pub fn save_state(&self) {
        self.behavior.save_state();
    }

    // endregion
}

impl IComposite for Composite {
    fn on_did_focus(&self) -> Event<()> {
        self.focus_events().map_or_else(Event::none, |(on_did_focus, _)| on_did_focus.event())
    }

    fn on_did_blur(&self) -> Event<()> {
        self.focus_events().map_or_else(Event::none, |(_, on_did_blur)| on_did_blur.event())
    }

    fn has_focus(&self) -> bool {
        self.has_focus.get()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_title(&self) -> Option<String> {
        self.behavior.get_title()
    }

    fn get_control(&self) -> Option<Rc<dyn ICompositeControl>> {
        self.behavior.get_control()
    }

    fn focus(&self) {
        self.behavior.focus();
    }
}

impl IDisposable for Composite {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O que um produto de registro precisa dar ao `CompositePart`: o [`Composite`] que ele é (ou que o estende por composição).
pub trait CompositeInstance {
    /// O composite do produto.
    fn composite(&self) -> Rc<Composite>;
}

impl CompositeInstance for Composite {
    fn composite(&self) -> Rc<Composite> {
        assert_returns_defined(self.weak_self.upgrade())
    }
}

/// `IConstructorSignature<T>`: `instantiationService.createInstance(ctor)`.
pub type CompositeFactory<T> = Rc<dyn Fn(&Rc<InstantiationService>) -> Rc<T>>;

/// A composite descriptor is a lightweight descriptor of a composite in the workbench.
pub struct CompositeDescriptor<T: ?Sized + 'static> {
    ctor: CompositeFactory<T>,
    pub id: String,
    pub name: String,
    pub css_class: Option<String>,
    pub order: Option<f64>,
    pub requested_index: Option<f64>,
}

impl<T: ?Sized + 'static> Clone for CompositeDescriptor<T> {
    fn clone(&self) -> Self {
        Self {
            ctor: self.ctor.clone(),
            id: self.id.clone(),
            name: self.name.clone(),
            css_class: self.css_class.clone(),
            order: self.order,
            requested_index: self.requested_index,
        }
    }
}

impl<T: ?Sized + 'static> CompositeDescriptor<T> {
    /// `new CompositeDescriptor(ctor, id, name, cssClass?, order?, requestedIndex?)`.
    pub fn new(ctor: CompositeFactory<T>, id: &str, name: &str, css_class: Option<&str>, order: Option<f64>, requested_index: Option<f64>) -> Self {
        Self { ctor, id: id.to_owned(), name: name.to_owned(), css_class: css_class.map(str::to_owned), order, requested_index }
    }

    /// A identidade do descritor (o `===` do original): o descritor registrado é o mesmo quando o clone devolvido pelo registro
    /// compartilha a fábrica.
    pub fn is_same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.ctor, &other.ctor)
    }

    /// `instantiate(instantiationService)`.
    pub fn instantiate(&self, instantiation_service: &Rc<InstantiationService>) -> Rc<T> {
        (self.ctor)(instantiation_service)
    }
}

/// O que o `CompositePart` lê do descritor de um identificador: o nome e a fábrica do [`Composite`].
#[derive(Clone)]
pub struct CompositeEntry {
    pub id: String,
    pub name: String,
    instantiate: CompositeFactory<Composite>,
}

impl CompositeEntry {
    /// `descriptor.instantiate(instantiationService)`.
    pub fn instantiate(&self, instantiation_service: &Rc<InstantiationService>) -> Rc<Composite> {
        (self.instantiate)(instantiation_service)
    }
}

/// O `CompositeRegistry<T>` como o `CompositePart` o usa (`this.registry.getComposite(id)`).
pub trait ICompositeRegistry {
    /// `getComposite(id)`.
    fn get_composite_entry(&self, id: &str) -> Option<CompositeEntry>;
}

/// `CompositeRegistry<T>`.
pub struct CompositeRegistry<T: ?Sized + 'static> {
    base: Rc<Disposable>,
    on_did_register: Rc<Emitter<CompositeDescriptor<T>>>,
    on_did_deregister: Rc<Emitter<CompositeDescriptor<T>>>,
    composites: RefCell<Vec<CompositeDescriptor<T>>>,
}

impl<T: ?Sized + 'static> CompositeRegistry<T> {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_register = Emitter::new();
        base.register(on_did_register.clone());
        let on_did_deregister = Emitter::new();
        base.register(on_did_deregister.clone());
        Rc::new(Self { base, on_did_register, on_did_deregister, composites: RefCell::new(Vec::new()) })
    }

    /// `onDidRegister`.
    pub fn on_did_register(&self) -> Event<CompositeDescriptor<T>> {
        self.on_did_register.event()
    }

    /// `onDidDeregister`.
    pub fn on_did_deregister(&self) -> Event<CompositeDescriptor<T>> {
        self.on_did_deregister.event()
    }

    /// `registerComposite(descriptor)`: um identificador repetido é ignorado.
    pub fn register_composite(&self, descriptor: CompositeDescriptor<T>) {
        if self.composite_by_id(&descriptor.id).is_some() {
            return;
        }

        self.composites.borrow_mut().push(descriptor.clone());
        self.on_did_register.fire(descriptor);
    }

    /// `deregisterComposite(id)`.
    pub fn deregister_composite(&self, id: &str) {
        let removed = {
            let mut composites = self.composites.borrow_mut();
            composites.iter().position(|composite| composite.id == id).map(|index| composites.remove(index))
        };
        if let Some(descriptor) = removed {
            self.on_did_deregister.fire(descriptor);
        }
    }

    /// `getComposite(id)`.
    pub fn get_composite(&self, id: &str) -> Option<CompositeDescriptor<T>> {
        self.composite_by_id(id)
    }

    /// `getComposites()`: os descritores registrados, na ordem de registro.
    pub fn get_composites(&self) -> Vec<CompositeDescriptor<T>> {
        self.composites.borrow().clone()
    }

    fn composite_by_id(&self, id: &str) -> Option<CompositeDescriptor<T>> {
        self.composites.borrow().iter().find(|composite| composite.id == id).cloned()
    }
}

impl<T: CompositeInstance + ?Sized + 'static> ICompositeRegistry for CompositeRegistry<T> {
    fn get_composite_entry(&self, id: &str) -> Option<CompositeEntry> {
        let descriptor = self.composite_by_id(id)?;
        let (entry_id, name) = (descriptor.id.clone(), descriptor.name.clone());
        let instantiate: CompositeFactory<Composite> = Rc::new(move |instantiation_service: &Rc<InstantiationService>| descriptor.instantiate(instantiation_service).composite());
        Some(CompositeEntry { id: entry_id, name, instantiate })
    }
}

impl<T: ?Sized + 'static> IDisposable for CompositeRegistry<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
