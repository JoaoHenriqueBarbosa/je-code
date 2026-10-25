//! Porte de `src/vs/workbench/services/banner/browser/bannerService.ts`: o contrato `IBannerService` e o `IBannerItem`.
//!
//! Divergências:
//! - **`ILinkDescriptor`** (`platform/opener/browser/link.ts`) é [`ILinkDescriptor`] aqui: o `label` do original é
//!   `string | HTMLElement`, e a renderização é nativa (sem `HTMLElement`), então o rótulo é só o texto.
//! - **`icon: ThemeIcon | URI | undefined`** é [`BannerIcon`] dentro de um `Option`, e **`message: string |
//!   MarkdownString`** é [`BannerMessage`].
//! - **`onClose`** é um `Rc<dyn Fn()>`, para o item ser copiável (o banner guarda o item enquanto o mostra).

use std::rc::Rc;

use je_base::html_content::MarkdownString;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `ILinkDescriptor`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ILinkDescriptor {
    pub label: String,
    pub href: String,
    pub title: Option<String>,
    pub tab_index: Option<i32>,
}

/// `ThemeIcon | URI`.
#[derive(Clone, Debug, PartialEq)]
pub enum BannerIcon {
    Theme(ThemeIcon),
    Uri(URI),
}

/// `string | MarkdownString`.
#[derive(Clone)]
pub enum BannerMessage {
    Text(String),
    Markdown(MarkdownString),
}

impl BannerMessage {
    /// `String(message)`: o texto literal, ou o valor de um `MarkdownString`.
    pub fn to_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Markdown(markdown) => markdown.value.to_string_lossy(),
        }
    }
}

/// `IBannerItem`.
#[derive(Clone)]
pub struct IBannerItem {
    pub id: String,
    pub icon: Option<BannerIcon>,
    pub message: BannerMessage,
    pub actions: Option<Vec<ILinkDescriptor>>,
    pub aria_label: Option<String>,
    pub on_close: Option<Rc<dyn Fn()>>,
    pub close_label: Option<String>,
    /// Higher-priority banners are shown first. Equal priorities use the most recently shown item.
    pub priority: Option<i32>,
    pub neutral: Option<bool>,
}

/// `IBannerService`.
pub trait IBannerService {
    fn focus(&self);
    fn focus_next_action(&self);
    fn focus_previous_action(&self);
    fn hide(&self, id: &str);
    fn show(&self, item: IBannerItem);
}

service_id!(pub I_BANNER_SERVICE: dyn IBannerService = "bannerService");
