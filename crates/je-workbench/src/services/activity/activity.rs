//! Porte de `src/vs/workbench/services/activity/common/activity.ts`: o contrato [`IActivityService`] (selos de atividade
//! de contêineres de views, de views, das contas e globais), com [`IActivity`], [`IBadge`], [`IBadgeStyles`] e os selos
//! [`NumberBadge`], [`IconBadge`], [`ProgressBadge`], [`WarningBadge`] e [`ErrorBadge`].
//!
//! Divergências:
//! - `onDidChangeActivity: Event<string | ViewContainer>` é `Event<`[`ActivityChangeTarget`]`>`, a união como enum.
//! - O `badge instanceof ProgressBadge`, `instanceof NumberBadge ? badge.number` e `instanceof IconBadge ? badge.icon`
//!   do `CompositeBarActionViewItem` e do `ScmMultiDiffSourceResolver` são `IBadge::is_progress_badge`, `IBadge::number`
//!   e `IBadge::icon` (Rust não tem `instanceof` sobre um `dyn`).
//! - `BaseBadge<T>` é um tipo interno que os selos guardam (não há herança). `WarningBadge` e `ErrorBadge` estendem
//!   `IconBadge` só para fixar o ícone e a função de estilos; aqui `WarningBadge::new` e `ErrorBadge::new` devolvem o
//!   próprio [`IconBadge`] assim montado, que é o que qualquer `instanceof IconBadge` do original enxerga.
//! - `IActivity` do original só tem o `badge`; o `priority` que o `CustomTreeView` põe no objeto literal é ignorado pelo
//!   serviço (a variável do original não passa pela checagem de propriedade extra), então não há campo aqui. Como o
//!   [`IActivity`] aqui é um valor (`Clone`), a identidade do objeto do original (`activities.indexOf(activity)`) é a do
//!   `Rc` do selo.
//! - O `Color` do `IBadgeStyles` é o do `je-theme` e o `IColorTheme` é o de `je_editor::services::theme_service`.

use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_base::themables::ThemeIcon;
use je_editor::services::theme_service::IColorTheme;
use je_theme::color::Color;

use crate::common::views::ViewContainerRc;

/// `activityWarningBadgeForeground` e `activityWarningBadgeBackground` (os identificadores das cores registradas em
/// `miscColors.ts`).
pub const ACTIVITY_WARNING_BADGE_FOREGROUND: &str = "activityWarningBadge.foreground";
pub const ACTIVITY_WARNING_BADGE_BACKGROUND: &str = "activityWarningBadge.background";

/// `activityErrorBadgeForeground` e `activityErrorBadgeBackground`.
pub const ACTIVITY_ERROR_BADGE_FOREGROUND: &str = "activityErrorBadge.foreground";
pub const ACTIVITY_ERROR_BADGE_BACKGROUND: &str = "activityErrorBadge.background";

/// `IBadgeStyles`.
#[derive(Clone, Debug, PartialEq)]
pub struct IBadgeStyles {
    pub badge_background: Option<Color>,
    pub badge_foreground: Option<Color>,
    pub badge_border: Option<Color>,
}

/// `(theme: IColorTheme) => IBadgeStyles | undefined`: a função de estilos de um selo.
pub type BadgeStylesFn = Rc<dyn Fn(&dyn IColorTheme) -> Option<IBadgeStyles>>;

/// `IBadge`.
pub trait IBadge {
    fn get_description(&self) -> String;

    /// `getColors(theme)`.
    fn get_colors(&self, theme: &dyn IColorTheme) -> Option<IBadgeStyles>;

    /// O `badge instanceof ProgressBadge` do original: só o [`ProgressBadge`] responde `true`.
    fn is_progress_badge(&self) -> bool {
        false
    }

    /// O `badge instanceof NumberBadge ? badge.number : ...` do original: só o [`NumberBadge`] responde o número.
    fn number(&self) -> Option<f64> {
        None
    }

    /// O `badge instanceof IconBadge ? badge.icon : ...` do original: só o [`IconBadge`] responde o ícone.
    fn icon(&self) -> Option<ThemeIcon> {
        None
    }
}

/// `IActivity`.
#[derive(Clone)]
pub struct IActivity {
    pub badge: Rc<dyn IBadge>,
}

/// `BaseBadge<T>`: a descrição (função do argumento `T`) e a função de estilos opcional.
struct BaseBadge<T> {
    descriptor_fn: Rc<dyn Fn(T) -> String>,
    styles_fn: Option<BadgeStylesFn>,
}

impl<T> BaseBadge<T> {
    fn new(descriptor_fn: impl Fn(T) -> String + 'static, styles_fn: Option<BadgeStylesFn>) -> Self {
        Self { descriptor_fn: Rc::new(descriptor_fn), styles_fn }
    }

    /// `this.descriptorFn(arg)`.
    fn describe(&self, arg: T) -> String {
        (self.descriptor_fn)(arg)
    }

    /// `getColors(theme)`: `this.stylesFn?.(theme)`.
    fn get_colors(&self, theme: &dyn IColorTheme) -> Option<IBadgeStyles> {
        self.styles_fn.as_ref().and_then(|styles_fn| styles_fn(theme))
    }
}

/// `NumberBadge`.
pub struct NumberBadge {
    pub number: f64,
    base: BaseBadge<f64>,
}

impl NumberBadge {
    /// `new NumberBadge(number, descriptorFn)`.
    pub fn new(number: f64, descriptor_fn: impl Fn(f64) -> String + 'static) -> Self {
        Self { number, base: BaseBadge::new(descriptor_fn, None) }
    }
}

impl IBadge for NumberBadge {
    fn get_description(&self) -> String {
        self.base.describe(self.number)
    }

    fn get_colors(&self, theme: &dyn IColorTheme) -> Option<IBadgeStyles> {
        self.base.get_colors(theme)
    }

    fn number(&self) -> Option<f64> {
        Some(self.number)
    }
}

/// `IconBadge`: o selo que mostra um ícone no lugar de um número.
pub struct IconBadge {
    pub icon: ThemeIcon,
    base: BaseBadge<()>,
}

impl IconBadge {
    /// `new IconBadge(icon, descriptorFn, stylesFn?)`.
    pub fn new(icon: ThemeIcon, descriptor_fn: impl Fn() -> String + 'static, styles_fn: Option<BadgeStylesFn>) -> Self {
        Self { icon, base: BaseBadge::new(move |()| descriptor_fn(), styles_fn) }
    }
}

impl IBadge for IconBadge {
    fn get_description(&self) -> String {
        self.base.describe(())
    }

    fn get_colors(&self, theme: &dyn IColorTheme) -> Option<IBadgeStyles> {
        self.base.get_colors(theme)
    }

    fn icon(&self) -> Option<ThemeIcon> {
        Some(self.icon.clone())
    }
}

/// `ProgressBadge`: o selo de progresso (o `descriptorFn` é o que o construtor recebe).
pub struct ProgressBadge {
    base: BaseBadge<()>,
}

impl ProgressBadge {
    /// `new ProgressBadge(descriptorFn)`.
    pub fn new(descriptor_fn: impl Fn() -> String + 'static) -> Self {
        Self { base: BaseBadge::new(move |()| descriptor_fn(), None) }
    }
}

impl IBadge for ProgressBadge {
    fn get_description(&self) -> String {
        self.base.describe(())
    }

    fn get_colors(&self, theme: &dyn IColorTheme) -> Option<IBadgeStyles> {
        self.base.get_colors(theme)
    }

    fn is_progress_badge(&self) -> bool {
        true
    }
}

/// O `IconBadge` com um codicon e as cores de fundo e de texto lidas do tema (sem borda): o que `WarningBadge` e
/// `ErrorBadge` têm em comum.
fn themed_icon_badge(codicon: &str, background: &'static str, foreground: &'static str, descriptor_fn: impl Fn() -> String + 'static) -> IconBadge {
    let styles_fn: BadgeStylesFn = Rc::new(move |theme: &dyn IColorTheme| -> Option<IBadgeStyles> {
        Some(IBadgeStyles { badge_background: theme.get_color(background, None), badge_foreground: theme.get_color(foreground, None), badge_border: None })
    });
    IconBadge::new(ThemeIcon::from_id(codicon), descriptor_fn, Some(styles_fn))
}

/// `WarningBadge`: o `IconBadge` de `Codicon.warning` com as cores `activityWarningBadge`.
pub struct WarningBadge;

impl WarningBadge {
    /// `new WarningBadge(descriptorFn)`.
    pub fn new(descriptor_fn: impl Fn() -> String + 'static) -> IconBadge {
        themed_icon_badge("warning", ACTIVITY_WARNING_BADGE_BACKGROUND, ACTIVITY_WARNING_BADGE_FOREGROUND, descriptor_fn)
    }
}

/// `ErrorBadge`: o `IconBadge` de `Codicon.error` com as cores `activityErrorBadge`.
pub struct ErrorBadge;

impl ErrorBadge {
    /// `new ErrorBadge(descriptorFn)`.
    pub fn new(descriptor_fn: impl Fn() -> String + 'static) -> IconBadge {
        themed_icon_badge("error", ACTIVITY_ERROR_BADGE_BACKGROUND, ACTIVITY_ERROR_BADGE_FOREGROUND, descriptor_fn)
    }
}

/// O `string | ViewContainer` de `onDidChangeActivity`: o identificador da ação global (`GLOBAL_ACTIVITY_ID`,
/// `ACCOUNTS_ACTIVITY_ID`) ou o contêiner de views cuja atividade mudou.
#[derive(Clone)]
pub enum ActivityChangeTarget {
    Id(String),
    ViewContainer(ViewContainerRc),
}

/// `IActivityService`.
pub trait IActivityService {
    /// `onDidChangeActivity`: dispara quando a atividade de um contêiner de views ou das ações globais muda.
    fn on_did_change_activity(&self) -> je_base::event::Event<ActivityChangeTarget>;

    /// `showViewContainerActivity(viewContainerId, badge)`: mostra o selo do contêiner de views até o descarte do retorno.
    fn show_view_container_activity(&self, view_container_id: &str, badge: IActivity) -> DisposableRef;

    /// `getViewContainerActivities(viewContainerId)`: as atividades do contêiner de views.
    fn get_view_container_activities(&self, view_container_id: &str) -> Vec<IActivity>;

    /// `showViewActivity(viewId, badge)`: mostra o selo da view até o descarte do retorno.
    fn show_view_activity(&self, view_id: &str, badge: IActivity) -> DisposableRef;

    /// `showAccountsActivity(activity)`: mostra o selo do menu de contas até o descarte do retorno.
    fn show_accounts_activity(&self, activity: IActivity) -> DisposableRef;

    /// `showGlobalActivity(activity)`: mostra o selo da ação global (engrenagem) até o descarte do retorno.
    fn show_global_activity(&self, activity: IActivity) -> DisposableRef;

    /// `getActivity(id)`: as atividades da ação de identificador `id`.
    fn get_activity(&self, id: &str) -> Vec<IActivity>;
}

je_instantiation::delayed_proxy!(pub ActivityServiceProxy for dyn IActivityService {
    methods {
        fn show_view_container_activity(&self, view_container_id: &str, badge: IActivity) -> DisposableRef;
        fn get_view_container_activities(&self, view_container_id: &str) -> Vec<IActivity>;
        fn show_view_activity(&self, view_id: &str, badge: IActivity) -> DisposableRef;
        fn show_accounts_activity(&self, activity: IActivity) -> DisposableRef;
        fn show_global_activity(&self, activity: IActivity) -> DisposableRef;
        fn get_activity(&self, id: &str) -> Vec<IActivity>;
    }
    events {
        fn on_did_change_activity(&self) -> ActivityChangeTarget;
    }
});

je_instantiation::service_id!(pub I_ACTIVITY_SERVICE: dyn IActivityService = "activityService", proxy ActivityServiceProxy);
