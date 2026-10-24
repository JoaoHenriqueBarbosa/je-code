//! Porte de `src/vs/workbench/contrib/terminal/browser/widgets/terminalHoverWidget.ts`: o hover dos links do terminal, na parte
//! lógica (o conteúdo, os alvos por linha de células, o estado dos modificadores). A pintura fica com o frontend nativo.
//!
//! Decisões de tradução:
//! - **`IHoverService.showInstantHover({ target, content, actions, linkHandler, additionalClasses })`** é o
//!   [`ITerminalHoverService`], que o frontend implementa: recebe os retângulos de alvo ([`HoverTargetRect`]) no lugar dos
//!   elementos `div.terminal-hover-target`, e as mesmas ações, conteúdo e tratador de link.
//! - **`CellHoverTarget`** calcula os mesmos retângulos (esquerda, base, largura e altura em pixels CSS) das até três linhas
//!   de alvo (topo, meio e fim) a partir do intervalo da viewport e das dimensões de célula e de terminal. Os `style.left`,
//!   `style.bottom`, `style.width` e `style.height` do original são os campos do retângulo.
//! - **`document.addEventListener('keydown'/'keyup')`** do `CellHoverTarget` são os eventos do [`ITerminalDocumentEvents`]
//!   do contêiner. Como o original só descarta o `div` e os ouvintes no `dispose` do alvo (que o serviço de hover chama),
//!   os ouvintes ficam registrados no `Disposable` do próprio widget, que o `dispose` do widget descarta.
//! - **`IConfigurationService.getValue(TerminalSettingId.ShowLinkHover)`** é lido em `attach`, como no original.

use std::rc::Rc;

use je_base::html_content::MarkdownString;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_platform::configuration::service::IConfigurationService;
use je_platform::terminal::terminal_settings::TerminalSettingId;
use je_widgets::hover_api::IHoverAction;

use super::widgets::ITerminalWidget;
use crate::contrib::terminal::browser::terminal_input_events::ITerminalDocumentEvents;
use crate::contrib::terminal::xterm_buffer::IViewportRange;

/// `{ width, height }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dimensions {
    pub width: f64,
    pub height: f64,
}

/// `ILinkHoverTargetOptions`.
#[derive(Clone)]
pub struct ILinkHoverTargetOptions {
    pub viewport_range: IViewportRange,
    pub cell_dimensions: Dimensions,
    pub terminal_dimensions: Dimensions,
    pub modifier_down_callback: Option<Rc<dyn Fn()>>,
    pub modifier_up_callback: Option<Rc<dyn Fn()>>,
}

/// O retângulo de um `div.terminal-hover-target`, em pixels CSS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverTargetRect {
    pub left: f64,
    pub bottom: f64,
    pub width: f64,
    pub height: f64,
}

/// As opções de `IHoverService.showInstantHover` que o hover do terminal usa.
#[derive(Clone)]
pub struct TerminalHoverOptions {
    pub target_rects: Vec<HoverTargetRect>,
    pub content: MarkdownString,
    pub actions: Option<Vec<IHoverAction>>,
    pub link_handler: Rc<dyn Fn(&str)>,
    pub additional_classes: Vec<String>,
}

/// `IHoverService`, no que o hover do terminal usa.
pub trait ITerminalHoverService {
    /// `showInstantHover(options)`: o hover mostrado, ou `None`.
    fn show_instant_hover(&self, options: TerminalHoverOptions) -> Option<DisposableRef>;
}

/// Os retângulos de alvo de um `CellHoverTarget` (o topo, o meio e o fim).
pub fn cell_hover_target_rects(options: &ILinkHoverTargetOptions) -> Vec<HoverTargetRect> {
    let range = &options.viewport_range;
    let (cell, terminal) = (options.cell_dimensions, options.terminal_dimensions);
    let (start_x, start_y, end_x, end_y) = (f64::from(range.start.x), f64::from(range.start.y), f64::from(range.end.x), f64::from(range.end.y));
    let row_count = range.end.y - range.start.y + 1;
    let mut rects = Vec::new();

    // Add top target row
    let width = (if range.end.y > range.start.y { terminal.width - start_x } else { end_x - start_x + 1.0 }) * cell.width;
    rects.push(HoverTargetRect {
        left: start_x * cell.width,
        bottom: (terminal.height - start_y - 1.0) * cell.height,
        width,
        height: cell.height,
    });

    // Add middle target rows
    if row_count > 2 {
        let middle_rows = f64::from(row_count - 2);
        rects.push(HoverTargetRect {
            left: 0.0,
            bottom: (terminal.height - start_y - 1.0 - middle_rows) * cell.height,
            width: terminal.width * cell.width,
            height: middle_rows * cell.height,
        });
    }

    // Add bottom target row
    if row_count > 1 {
        rects.push(HoverTargetRect {
            left: 0.0,
            bottom: (terminal.height - end_y - 1.0) * cell.height,
            width: (end_x + 1.0) * cell.width,
            height: cell.height,
        });
    }
    rects
}

/// `TerminalHover`.
pub struct TerminalHover {
    base: Rc<Disposable>,
    target_options: ILinkHoverTargetOptions,
    text: MarkdownString,
    actions: Option<Vec<IHoverAction>>,
    link_handler: Rc<dyn Fn(&str)>,
    hover_service: Rc<dyn ITerminalHoverService>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl TerminalHover {
    pub const ID: &'static str = "hover";

    pub fn new(
        target_options: ILinkHoverTargetOptions,
        text: MarkdownString,
        actions: Option<Vec<IHoverAction>>,
        link_handler: Rc<dyn Fn(&str)>,
        hover_service: Rc<dyn ITerminalHoverService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), target_options, text, actions, link_handler, hover_service, configuration_service })
    }

    /// O `CellHoverTarget`: os ouvintes de modificador do documento (`ctrlKey` desce e sobe).
    fn register_modifier_listeners(&self, container: &Rc<dyn ITerminalDocumentEvents>) {
        let (Some(modifier_down), Some(modifier_up)) = (self.target_options.modifier_down_callback.clone(), self.target_options.modifier_up_callback.clone()) else {
            return;
        };
        let down = Rc::new(std::cell::Cell::new(false));
        let down_state = down.clone();
        self.base.register(container.on_key_down().listen(move |event| {
            if event.ctrl_key && !down_state.get() {
                down_state.set(true);
                modifier_down();
            }
        }));
        self.base.register(container.on_key_up().listen(move |event| {
            if !event.ctrl_key {
                down.set(false);
                modifier_up();
            }
        }));
    }
}

impl IDisposable for TerminalHover {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITerminalWidget for TerminalHover {
    fn id(&self) -> String {
        Self::ID.to_owned()
    }

    fn attach(&self, container: Rc<dyn ITerminalDocumentEvents>) {
        let show_link_hover = self.configuration_service.get_value(Some(TerminalSettingId::ShowLinkHover.as_str()), None);
        if !show_link_hover.is_truthy() {
            return;
        }
        self.register_modifier_listeners(&container);
        let hover = self.hover_service.show_instant_hover(TerminalHoverOptions {
            target_rects: cell_hover_target_rects(&self.target_options),
            content: self.text.clone(),
            actions: self.actions.clone(),
            link_handler: self.link_handler.clone(),
            // .xterm-hover lets xterm know that the hover is part of a link
            additional_classes: vec!["xterm-hover".to_owned()],
        });
        if let Some(hover) = hover {
            self.base.register(hover);
        }
    }
}
