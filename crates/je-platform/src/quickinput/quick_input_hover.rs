//! Porte do `QuickInputHoverDelegate` de `src/vs/platform/quickinput/browser/quickInput.ts`: o `WorkbenchHoverDelegate` com o
//! mouse como posição, que não fecha o hover ao teclar, pula a animação de entrada e só mostra a dica de "mais" quando o
//! conteúdo tem mais de uma linha.
//!
//! No original é uma subclasse sem membros novos; aqui [`QuickInputHoverDelegate::new`] devolve o próprio
//! [`WorkbenchHoverDelegate`] configurado.

use std::rc::Rc;

use crate::configuration::service::IConfigurationService;
use crate::hover::hover_service::{HoverContent, HoverPlacement, HoverService, IHoverAppearanceOptions, IHoverDom, IHoverOptions, IHoverPersistenceOptions};
use crate::hover::workbench_hover_delegate::{HoverOverrideOptions, HoverOverrides, WorkbenchHoverDelegate};

pub struct QuickInputHoverDelegate;

impl QuickInputHoverDelegate {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>, hover_service: Rc<HoverService>, dom: Rc<dyn IHoverDom>) -> Rc<WorkbenchHoverDelegate> {
        WorkbenchHoverDelegate::new(
            HoverPlacement::Mouse,
            None,
            HoverOverrideOptions::Dynamic(Rc::new(|options, _focus| Self::override_options(options))),
            configuration_service,
            hover_service,
            dom,
        )
    }

    /// `getOverrideOptions(options)`.
    fn override_options(options: &IHoverOptions) -> HoverOverrides {
        // Only show the hover hint if the content is of a decent size. Um elemento não carrega texto aqui (`textContent ?? ''`).
        let show_hover_hint = match &options.content {
            HoverContent::Element(_) => false,
            HoverContent::Text(text) => text.contains('\n'),
            HoverContent::Markdown(markdown) => markdown.value.to_string().contains('\n'),
        };
        HoverOverrides {
            persistence: Some(IHoverPersistenceOptions { hide_on_key_down: Some(false), ..Default::default() }),
            appearance: Some(IHoverAppearanceOptions { show_hover_hint: Some(show_hover_hint), skip_fade_in_animation: Some(true), ..Default::default() }),
            ..Default::default()
        }
    }
}
