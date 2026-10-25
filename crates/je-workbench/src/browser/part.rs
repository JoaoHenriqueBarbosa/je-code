//! Porte de `src/vs/workbench/browser/part.ts` (e do `Component` de `common/component.ts` e do `Themable` de
//! `platform/theme/common/themeService.ts` que ele estende): a [`Part`], o `PartLayout` do título, do cabeçalho, do
//! conteúdo e do rodapé, e as opções ([`PartOptions`]).
//!
//! Divergências:
//! - **Herança vira composição.** A classe abstrata `Part` que as partes estendem é a [`Part`] concreta, e o que as subclasses
//!   sobrescrevem (`createTitleArea`, `createContentArea`, `updateStyles`, `layout`, `getRelayoutDimension`, `saveState`, os
//!   limites `minimumWidth`/`maximumWidth`/`minimumHeight`/`maximumHeight` e o `toJSON`) é o trait [`PartBehavior`]. A chamada
//!   a `super.layout(...)` de uma sobrescrita é [`Part::layout_base`]; o que o original deixa `protected` (`layoutContents`,
//!   `setHeaderArea`, `setFooterArea`, `removeHeaderArea`, `removeFooterArea`, `getMemento`, `saveState`) é público, porque
//!   quem estende a parte é um [`PartBehavior`] que recebe a `&Part`.
//! - **DOM vira estado nativo.** Os elementos são [`ViewElement`] (pai, classes, tamanho). `prepend`, `appendChild` e `remove`
//!   do cabeçalho e do rodapé são os métodos `prepend_child`, `append_child` e `remove_child` do [`PartBehavior`], que por
//!   padrão só trocam o pai; quem desenha a ordem dos filhos os sobrescreve.
//! - **`ISerializableView.element`** é atribuído pelas subclasses e não é lido pela `Part`; não entra aqui.
//! - **`Component.memento`.** O `Memento` é o de `common/memento`; o `onWillSaveState` pede o `saveState` da parte e grava o
//!   memento, como o `Component`.
//! - **`Themable`.** O `theme` é o [`PartTheme`](crate::browser::parts::part_support::PartTheme); `onThemeChange` só age depois
//!   do `create`, como o `Part.onThemeChange`.
//! - **`MultiWindowParts`** depende de `getActiveDocument` (a janela ativa) e fica para o porte do serviço de partes
//!   multijanela.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::storage::{StorageScope, StorageService, StorageTarget, StorageValueChangeEvent};
use je_theme::color::Color;
use je_widgets::gridview::ViewDimensions;

use crate::browser::parts::part_support::{ColorModifier, PartTheme};
use crate::common::memento::Memento;
use crate::services::layout::layout_service::{IWorkbenchLayoutService, LayoutPart};

/// `Dimension.None`: a dimensão vazia.
pub const NONE_DIMENSION: Dimension = Dimension { width: 0.0, height: 0.0 };

/// `IPartOptions`.
#[derive(Clone, Default)]
pub struct PartOptions {
    /// `hasTitle` (ausente é falso).
    pub has_title: bool,
    /// `borderWidth`.
    pub border_width: Option<Rc<dyn Fn() -> f64>>,
    /// `titleHeight`: devolve `None` quando a parte não impõe uma altura.
    pub title_height: Option<Rc<dyn Fn() -> Option<f64>>>,
}

/// `ILayoutContentResult`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutContentResult {
    pub header_size: Dimension,
    pub title_size: Dimension,
    pub content_size: Dimension,
    pub footer_size: Dimension,
}

/// `IDomPosition`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DomPosition {
    pub top: f64,
    pub left: f64,
}

/// O que as subclasses de `Part` sobrescrevem.
pub trait PartBehavior {
    /// `createTitleArea(parent, options)`: o padrão não cria título.
    fn create_title_area(&self, _parent: &Rc<ViewElement>, _options: Option<&Dynamic>) -> Option<Rc<ViewElement>> {
        None
    }

    /// `createContentArea(parent, options)`: o padrão não cria conteúdo.
    fn create_content_area(&self, _parent: &Rc<ViewElement>, _options: Option<&Dynamic>) -> Option<Rc<ViewElement>> {
        None
    }

    /// `updateStyles()`.
    fn update_styles(&self, _part: &Part) {}

    /// `layout(width, height, top, left)`: o padrão é o `Part.layout`.
    fn layout(&self, part: &Part, width: f64, height: f64, top: f64, left: f64) {
        part.layout_base(width, height, top, left);
    }

    /// `getRelayoutDimension()`: o padrão é a dimensão guardada pelo último `layout`.
    fn relayout_dimension(&self, part: &Part) -> Option<Dimension> {
        part.dimension()
    }

    /// `saveState()`: as subclasses guardam o estado no memento.
    fn save_state(&self) {}

    /// `minimumWidth`.
    fn minimum_width(&self) -> f64;

    /// `maximumWidth`.
    fn maximum_width(&self) -> f64;

    /// `minimumHeight`.
    fn minimum_height(&self) -> f64;

    /// `maximumHeight`.
    fn maximum_height(&self) -> f64;

    /// `toJSON()`.
    fn to_json(&self) -> Dynamic;

    /// `prepend(parent, child)`.
    fn prepend_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }

    /// `parent.appendChild(child)`.
    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }

    /// `child.remove()`.
    fn remove_child(&self, child: &Rc<ViewElement>) {
        child.set_parent(None);
    }
}

/// `assertReturnsDefined`: o `Error` do original quando o valor falta.
fn assert_returns_defined<T>(value: Option<T>) -> T {
    value.unwrap_or_else(|| errors::throw(Error::new("Assertion Failed: argument is undefined or null")))
}

#[derive(Default)]
struct PartState {
    dimension: Option<Dimension>,
    content_position: Option<DomPosition>,
    parent: Option<Rc<ViewElement>>,
    header_area: Option<Rc<ViewElement>>,
    title_area: Option<Rc<ViewElement>>,
    content_area: Option<Rc<ViewElement>>,
    footer_area: Option<Rc<ViewElement>>,
    part_layout: Option<Rc<PartLayout>>,
}

/// `Part`: partes são dispostas no workbench e têm um layout próprio que arranja um título opcional e uma área de conteúdo
/// obrigatória.
pub struct Part {
    id: String,
    options: PartOptions,
    behavior: Rc<dyn PartBehavior>,
    theme: PartTheme,
    layout_service: Rc<dyn IWorkbenchLayoutService>,
    memento: Memento,
    base: Rc<Disposable>,
    on_did_visibility_change: Rc<Emitter<bool>>,
    on_did_change: Rc<Emitter<Option<ViewDimensions>>>,
    state: RefCell<PartState>,
}

impl Part {
    /// `new Part(id, options, themeService, storageService, layoutService)`, com o que a subclasse sobrescreve em `behavior`.
    pub fn new(
        id: &str,
        options: PartOptions,
        theme_service: Rc<dyn IThemeService>,
        storage_service: Rc<dyn StorageService>,
        layout_service: Rc<dyn IWorkbenchLayoutService>,
        behavior: Rc<dyn PartBehavior>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_visibility_change: Rc<Emitter<bool>> = Emitter::new();
        let on_did_change: Rc<Emitter<Option<ViewDimensions>>> = Emitter::new();
        base.register(on_did_visibility_change.clone());
        base.register(on_did_change.clone());

        let this = Rc::new(Self {
            id: id.to_owned(),
            options,
            behavior,
            theme: PartTheme::new(theme_service.get_color_theme()),
            layout_service,
            memento: Memento::new(id, storage_service.clone()),
            base,
            on_did_visibility_change,
            on_did_change,
            state: RefCell::new(PartState::default()),
        });

        // Themable: `this._register(this.themeService.onDidColorThemeChange(theme => this.onThemeChange(theme)))`.
        let weak = Rc::downgrade(&this);
        this.base.register(theme_service.on_did_color_theme_change().listen(move |theme| {
            if let Some(this) = weak.upgrade() {
                this.on_theme_change(theme.clone());
            }
        }));

        // Component: `storageService.onWillSaveState`: pede à parte o estado e grava o memento.
        let weak = Rc::downgrade(&this);
        this.base.register(storage_service.on_will_save_state().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                // Ask the component to persist state into the memento
                this.save_state();

                // Then save the memento into storage
                this.memento.save_memento();
            }
        }));

        // Part: `this._register(layoutService.registerPart(this))`.
        let registration = this.layout_service.register_part(this.clone());
        this.base.register(registration);

        this
    }

    /// `getId()`.
    pub fn get_id(&self) -> &str {
        &self.id
    }

    /// `dimension`.
    pub fn dimension(&self) -> Option<Dimension> {
        self.state.borrow().dimension
    }

    /// `contentPosition`.
    pub fn content_position(&self) -> Option<DomPosition> {
        self.state.borrow().content_position
    }

    /// `titleArea`.
    pub fn title_area(&self) -> Option<Rc<ViewElement>> {
        self.state.borrow().title_area.clone()
    }

    /// `contentArea`.
    pub fn content_area(&self) -> Option<Rc<ViewElement>> {
        self.state.borrow().content_area.clone()
    }

    /// `onDidVisibilityChange`.
    pub fn on_did_visibility_change(&self) -> Event<bool> {
        self.on_did_visibility_change.event()
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.on_did_change.event()
    }

    /// `this.theme`.
    pub fn theme(&self) -> &PartTheme {
        &self.theme
    }

    /// `getColor(id, modify)`.
    pub fn get_color(&self, id: &str, modify: Option<ColorModifier<'_>>) -> Option<Color> {
        self.theme.get_color(id, modify)
    }

    /// `layoutService`.
    pub fn layout_service(&self) -> &Rc<dyn IWorkbenchLayoutService> {
        &self.layout_service
    }

    /// `Part.onThemeChange`: só depois do `create` (`Themable.onThemeChange` troca o tema e atualiza os estilos).
    fn on_theme_change(&self, theme: Rc<dyn IColorTheme>) {
        // only call if our create() method has been called
        if self.state.borrow().parent.is_some() {
            self.theme.set(theme);
            self.update_styles();
        }
    }

    /// `updateStyles()`.
    pub fn update_styles(&self) {
        self.behavior.update_styles(self);
    }

    /// Note: Clients should not call this method, the workbench calls this method. Calling it otherwise may result in unexpected
    /// behavior.
    ///
    /// Called to create title and content area of the part.
    pub fn create(&self, parent: &Rc<ViewElement>, options: Option<&Dynamic>) {
        self.state.borrow_mut().parent = Some(parent.clone());

        let title_area = self.behavior.create_title_area(parent, options);
        self.state.borrow_mut().title_area = title_area;
        let content_area = self.behavior.create_content_area(parent, options);
        self.state.borrow_mut().content_area = content_area.clone();

        let part_layout = Rc::new(PartLayout::new(self.options.clone(), content_area, self.layout_service.clone()));
        self.state.borrow_mut().part_layout = Some(part_layout);

        self.update_styles();
    }

    /// `getContainer()`: o contêiner geral da parte.
    pub fn get_container(&self) -> Option<Rc<ViewElement>> {
        self.state.borrow().parent.clone()
    }

    /// `setHeaderArea(headerContainer)`.
    pub fn set_header_area(&self, header_container: &Rc<ViewElement>) {
        if self.state.borrow().header_area.is_some() {
            errors::throw(Error::new("Header already exists"));
        }

        let (parent, title_area, part_layout) = {
            let state = self.state.borrow();
            (state.parent.clone(), state.title_area.clone(), state.part_layout.clone())
        };
        let (Some(parent), Some(_)) = (parent, title_area) else {
            return;
        };

        self.behavior.prepend_child(&parent, header_container);
        header_container.class_list_add("header-or-footer");
        header_container.class_list_add("header");

        self.state.borrow_mut().header_area = Some(header_container.clone());
        if let Some(part_layout) = part_layout {
            part_layout.set_header_visibility(true);
        }
        self.relayout();
    }

    /// `setFooterArea(footerContainer)`.
    pub fn set_footer_area(&self, footer_container: &Rc<ViewElement>) {
        if self.state.borrow().footer_area.is_some() {
            errors::throw(Error::new("Footer already exists"));
        }

        let (parent, title_area, part_layout) = {
            let state = self.state.borrow();
            (state.parent.clone(), state.title_area.clone(), state.part_layout.clone())
        };
        let (Some(parent), Some(_)) = (parent, title_area) else {
            return;
        };

        self.behavior.append_child(&parent, footer_container);
        footer_container.class_list_add("header-or-footer");
        footer_container.class_list_add("footer");

        self.state.borrow_mut().footer_area = Some(footer_container.clone());
        if let Some(part_layout) = part_layout {
            part_layout.set_footer_visibility(true);
        }
        self.relayout();
    }

    /// `removeHeaderArea()`.
    pub fn remove_header_area(&self) {
        let (header_area, part_layout) = {
            let mut state = self.state.borrow_mut();
            (state.header_area.take(), state.part_layout.clone())
        };
        if let Some(header_area) = header_area {
            self.behavior.remove_child(&header_area);
            if let Some(part_layout) = part_layout {
                part_layout.set_header_visibility(false);
            }
            self.relayout();
        }
    }

    /// `removeFooterArea()`.
    pub fn remove_footer_area(&self) {
        let (footer_area, part_layout) = {
            let mut state = self.state.borrow_mut();
            (state.footer_area.take(), state.part_layout.clone())
        };
        if let Some(footer_area) = footer_area {
            self.behavior.remove_child(&footer_area);
            if let Some(part_layout) = part_layout {
                part_layout.set_footer_visibility(false);
            }
            self.relayout();
        }
    }

    fn relayout(&self) {
        let dimension = self.behavior.relayout_dimension(self);
        if let (Some(dimension), Some(position)) = (dimension, self.content_position()) {
            self.layout(dimension.width, dimension.height, position.top, position.left);
        }
    }

    /// Layout title and content area in the given dimension.
    pub fn layout_contents(&self, width: f64, height: f64) -> LayoutContentResult {
        let part_layout = assert_returns_defined(self.state.borrow().part_layout.clone());

        part_layout.layout(width, height)
    }

    // region: ISerializableView

    /// `minimumWidth`.
    pub fn minimum_width(&self) -> f64 {
        self.behavior.minimum_width()
    }

    /// `maximumWidth`.
    pub fn maximum_width(&self) -> f64 {
        self.behavior.maximum_width()
    }

    /// `minimumHeight`.
    pub fn minimum_height(&self) -> f64 {
        self.behavior.minimum_height()
    }

    /// `maximumHeight`.
    pub fn maximum_height(&self) -> f64 {
        self.behavior.maximum_height()
    }

    /// `layout(width, height, top, left)`, com a sobrescrita da subclasse.
    pub fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        self.behavior.layout(self, width, height, top, left);
    }

    /// `Part.layout`: guarda a dimensão e a posição do conteúdo (o `super.layout` das subclasses).
    pub fn layout_base(&self, width: f64, height: f64, top: f64, left: f64) {
        let mut state = self.state.borrow_mut();
        state.dimension = Some(Dimension { width, height });
        state.content_position = Some(DomPosition { top, left });
    }

    /// `setVisible(visible)`.
    pub fn set_visible(&self, visible: bool) {
        self.on_did_visibility_change.fire(visible);
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> Dynamic {
        self.behavior.to_json()
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

impl LayoutPart for Part {
    fn get_id(&self) -> String {
        self.id.clone()
    }
}

impl IDisposable for Part {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `PartLayout`.
struct PartLayout {
    options: PartOptions,
    content_area: Option<Rc<ViewElement>>,
    layout_service: Rc<dyn IWorkbenchLayoutService>,
    header_visible: Cell<bool>,
    footer_visible: Cell<bool>,
}

impl PartLayout {
    const HEADER_HEIGHT: f64 = 35.0;
    const TITLE_HEIGHT: f64 = 35.0;
    const FOOTER_HEIGHT: f64 = 35.0;
    // KEEP IN SYNC WITH: modernUI/browser/media/padding.css
    const AREA_HEIGHT_MODERN_UI: f64 = 32.0;

    fn new(options: PartOptions, content_area: Option<Rc<ViewElement>>, layout_service: Rc<dyn IWorkbenchLayoutService>) -> Self {
        Self { options, content_area, layout_service, header_visible: Cell::new(false), footer_visible: Cell::new(false) }
    }

    /// A altura de uma área (título, cabeçalho ou rodapé): a do modo flutuante do Modern UI ou a clássica.
    fn area_height(&self, classic: f64) -> f64 {
        if self.layout_service.is_floating_panels_enabled() { Self::AREA_HEIGHT_MODERN_UI } else { classic }
    }

    fn layout(&self, width: f64, height: f64) -> LayoutContentResult {
        // Title Size: Width (Fill), Height (Variable).
        let title_size = if self.options.has_title {
            let custom_height = self.options.title_height.as_ref().and_then(|title_height| title_height());
            let title_height = custom_height.unwrap_or_else(|| self.area_height(Self::TITLE_HEIGHT));
            Dimension { width, height: height.min(title_height) }
        } else {
            NONE_DIMENSION
        };

        // Header Size: Width (Fill), Height (Variable)
        let header_size = if self.header_visible.get() { Dimension { width, height: height.min(self.area_height(Self::HEADER_HEIGHT)) } } else { NONE_DIMENSION };

        // Footer Size: Width (Fill), Height (Variable)
        let footer_size = if self.footer_visible.get() { Dimension { width, height: height.min(self.area_height(Self::FOOTER_HEIGHT)) } } else { NONE_DIMENSION };

        let mut content_width = width;
        if let Some(border_width) = &self.options.border_width {
            content_width -= border_width(); // adjust for border size
        }

        // Content Size: Width (Fill), Height (Variable)
        let content_size = Dimension { width: content_width, height: height - title_size.height - header_size.height - footer_size.height };

        // Content
        if let Some(content_area) = &self.content_area {
            content_area.set_size(content_size.width, content_size.height);
        }

        LayoutContentResult { header_size, title_size, content_size, footer_size }
    }

    fn set_footer_visibility(&self, visible: bool) {
        self.footer_visible.set(visible);
    }

    fn set_header_visibility(&self, visible: bool) {
        self.header_visible.set(visible);
    }
}
