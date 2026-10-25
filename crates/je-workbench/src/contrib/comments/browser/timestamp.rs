//! Porte de `src/vs/workbench/contrib/comments/browser/timestamp.ts`: o `TimestampWidget`, o carimbo de data de um comentário (hora
//! relativa ou data completa, conforme `comments.useRelativeTime`).
//!
//! Divergências:
//! - **`Date`.** Uma data é o valor de tempo em milissegundos (`Option<f64>`, `undefined` é `None`). O original compara datas por
//!   identidade (`timestamp !== this._timestamp`) e todo chamador passa um `new Date(...)` novo, então uma data presente é sempre
//!   "diferente" da anterior (e refaz o texto, que atualiza a hora relativa); duas ausências são iguais. [`TimestampWidget::set_timestamp`]
//!   faz o mesmo.
//! - **`async setTimestamp`.** O corpo não espera nada; é uma função comum.
//! - **`span.timestamp`** é um nó `timestamp` com um [`HighlightedLabel`] dentro (o `textContent`), que corta com reticências na largura
//!   que [`TimestampWidget::layout`] recebe (o `overflow: hidden` com `text-overflow: ellipsis` do CSS). `display: none` é o nó
//!   invisível. Os estilos do rótulo (fonte e cor) são o argumento `styles` de [`TimestampWidget::new`], porque o CSS os herda do
//!   contêiner.
//! - **`IHoverService`.** O hover é o do serviço de base de `je-widgets` (o serviço de hover do porte é global).
//! - **`toLocaleString(language)`.** É o formato `M/D/AAAA, h:mm:ss AM` da localidade `en-US` no fuso do sistema, o mesmo que o porte do
//!   terminal usa (o `Intl.DateTimeFormat` ainda não existe em `je-base`).
//! - **`getValue<ICommentsConfiguration>(COMMENTS_SECTION).useRelativeTime`** sem a seção `comments` lançaria `TypeError`; aqui vale
//!   falso.

use std::cell::Cell;
use std::rc::Rc;

use je_base::date::from_now;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_platform::configuration::service::IConfigurationService;
use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::{HighlightedLabel, HighlightedLabelStyles};
use je_widgets::hover_api::{IManagedHover, ManagedHoverContentOrFactory};

use crate::contrib::comments::common::comments_configuration::{COMMENTS_SECTION, ICommentsConfiguration};
use crate::contrib::markers::markers_tree_viewer::{place, setup_mouse_hover};
use crate::contrib::terminal::browser::xterm::decoration_styles::to_locale_string;

/// `TimestampWidget`.
pub struct TimestampWidget {
    base: Rc<Disposable>,
    ui: Ui,
    configuration_service: Rc<dyn IConfigurationService>,
    date: NodeId,
    label: Rc<HighlightedLabel>,
    timestamp: Cell<Option<f64>>,
    use_relative_time: Cell<bool>,
    hover: Rc<dyn IManagedHover>,
}

impl TimestampWidget {
    /// `new TimestampWidget(configurationService, hoverService, container, timeStamp?)`.
    pub fn new(ui: &Ui, configuration_service: Rc<dyn IConfigurationService>, container: NodeId, styles: HighlightedLabelStyles, time_stamp: Option<f64>) -> Rc<Self> {
        let date = create_el(&mut ui.tree_mut(), container, Role::Generic, "timestamp");
        ui.tree_mut().style_mut(date).visible = false;
        let label = HighlightedLabel::new(ui, date, None, styles);
        let hover = setup_mouse_hover(date, "");

        let base = Disposable::new();
        base.register(label.clone());
        {
            let hover = hover.clone();
            base.register(to_disposable(move || hover.dispose()));
        }

        let this = Rc::new(Self {
            base,
            ui: ui.clone(),
            configuration_service,
            date,
            label,
            timestamp: Cell::new(None),
            use_relative_time: Cell::new(false),
            hover,
        });
        this.use_relative_time.set(this.use_relative_time_setting());
        this.set_timestamp(time_stamp);
        this
    }

    /// O nó `span.timestamp`.
    pub fn element(&self) -> NodeId {
        self.date
    }

    /// O `display` do `span.timestamp` não é `none` (há uma data).
    pub fn is_visible(&self) -> bool {
        self.ui.tree().style(self.date).visible
    }

    /// A largura que o texto teria sem corte.
    pub fn natural_width(&self) -> f64 {
        self.label.set_max_width(None);
        self.label.width()
    }

    /// Posiciona o nó e o texto na origem do contêiner, cortando o texto em `max_width`, e devolve a largura usada (zero sem data).
    pub fn layout(&self, max_width: f64) -> f64 {
        if !self.is_visible() {
            return 0.0;
        }
        let max_width = max_width.max(0.0);
        self.label.set_max_width(Some(max_width));
        let width = self.label.width().min(max_width);
        let line_height = self.label.line_height();
        let mut tree = self.ui.tree_mut();
        place(&mut tree, self.date, 0.0, 0.0, width, line_height);
        place(&mut tree, self.label.element(), 0.0, 0.0, width, line_height);
        width
    }

    fn use_relative_time_setting(&self) -> bool {
        ICommentsConfiguration::from_dynamic(&self.configuration_service.get_value(Some(COMMENTS_SECTION), None)).is_some_and(|configuration| configuration.use_relative_time)
    }

    /// `timestamp !== this._timestamp`.
    fn differs_from_current(&self, timestamp: Option<f64>) -> bool {
        timestamp.is_some() || self.timestamp.get().is_some()
    }

    /// `setTimestamp(timestamp)`.
    pub fn set_timestamp(&self, timestamp: Option<f64>) {
        if self.differs_from_current(timestamp) || self.use_relative_time_setting() != self.use_relative_time.get() {
            self.update_date(timestamp);
        }
        self.timestamp.set(timestamp);
        self.use_relative_time.set(self.use_relative_time_setting());
    }

    fn update_date(&self, timestamp: Option<f64>) {
        match timestamp {
            None => {
                self.label.set(Some(""), &[], "", false, false);
                self.ui.tree_mut().style_mut(self.date).visible = false;
            }
            Some(timestamp) => {
                if self.differs_from_current(Some(timestamp)) || self.use_relative_time_setting() != self.use_relative_time.get() {
                    self.ui.tree_mut().style_mut(self.date).visible = true;
                    let (text_content, tooltip) = if self.use_relative_time_setting() {
                        (Self::get_relative(timestamp), Some(Self::get_date_string(timestamp)))
                    } else {
                        (Self::get_date_string(timestamp), None)
                    };

                    self.label.set(Some(&text_content), &[], "", false, false);
                    self.hover.update(ManagedHoverContentOrFactory::from(tooltip.as_deref().unwrap_or("")), None);
                }
            }
        }
    }

    fn get_relative(date: f64) -> String {
        from_now(Some(date), true, true, false)
    }

    fn get_date_string(date: f64) -> String {
        to_locale_string(date)
    }
}

impl IDisposable for TimestampWidget {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "TimestampWidget"
    }
}
