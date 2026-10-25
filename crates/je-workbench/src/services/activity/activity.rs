//! Porte parcial de `src/vs/workbench/services/activity/common/activity.ts`: o contrato [`IActivityService`] que o
//! `CustomTreeView` usa para mostrar o selo de uma view (`showViewActivity`), com [`IActivity`], [`IBadge`] e o
//! [`NumberBadge`].
//!
//! Divergências:
//! - `onDidChangeActivity` e `getViewContainerActivities` entram com implementação padrão (nunca dispara, lista vazia), e o
//!   `badge instanceof ProgressBadge` é `IBadge::is_progress_badge`.
//! - O resto da interface (`showViewContainerActivity`, `showGlobalActivity`, `getActivity`) e os
//!   selos `WarningBadge` e `ErrorBadge` pertencem às partes de barra de atividade e de contas, que ainda não foram portadas.
//!   O `showAccountsActivity` entra com o corpo padrão que lança (como os membros ausentes de `IViewsService`), porque a
//!   `EditSessionsContribution` o chama.
//! - `IBadge.getColors(theme)` fica de fora: `IColorTheme` e `Color` são da camada de tema da interface. Só
//!   `getDescription()` entra.
//! - `IActivity` do original só tem o `badge`; o `priority` que o `CustomTreeView` põe no objeto literal é ignorado pelo
//!   serviço (a variável do original não passa pela checagem de propriedade extra), então não há campo aqui.

use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::themables::ThemeIcon;

/// `IBadge`.
pub trait IBadge {
    fn get_description(&self) -> String;

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

/// `NumberBadge`.
pub struct NumberBadge {
    pub number: f64,
    descriptor_fn: Rc<dyn Fn(f64) -> String>,
}

impl NumberBadge {
    /// `new NumberBadge(number, descriptorFn)`.
    pub fn new(number: f64, descriptor_fn: impl Fn(f64) -> String + 'static) -> Self {
        Self { number, descriptor_fn: Rc::new(descriptor_fn) }
    }
}

impl IBadge for NumberBadge {
    fn get_description(&self) -> String {
        (self.descriptor_fn)(self.number)
    }

    fn number(&self) -> Option<f64> {
        Some(self.number)
    }
}

/// `IconBadge`: o selo que mostra um ícone no lugar de um número.
pub struct IconBadge {
    pub icon: ThemeIcon,
    description_fn: Rc<dyn Fn() -> String>,
}

impl IconBadge {
    /// `new IconBadge(icon, descriptorFn)`.
    pub fn new(icon: ThemeIcon, description_fn: impl Fn() -> String + 'static) -> Self {
        Self { icon, description_fn: Rc::new(description_fn) }
    }
}

impl IBadge for IconBadge {
    fn get_description(&self) -> String {
        (self.description_fn)()
    }

    fn icon(&self) -> Option<ThemeIcon> {
        Some(self.icon.clone())
    }
}

/// `ProgressBadge`: o selo de progresso (o `getDescription` é o que o construtor recebe).
pub struct ProgressBadge {
    description_fn: Rc<dyn Fn() -> String>,
}

impl ProgressBadge {
    /// `new ProgressBadge(getDescription)`.
    pub fn new(description_fn: impl Fn() -> String + 'static) -> Self {
        Self { description_fn: Rc::new(description_fn) }
    }
}

impl IBadge for ProgressBadge {
    fn get_description(&self) -> String {
        (self.description_fn)()
    }

    fn is_progress_badge(&self) -> bool {
        true
    }
}

/// O `string | IAction` de `onDidChangeActivity`: o identificador (do contêiner de views ou da ação global) ou a ação.
#[derive(Clone)]
pub enum ActivityChangeTarget {
    Id(String),
    Action(ActionRef),
}

/// `IActivityService`.
pub trait IActivityService {
    /// `onDidChangeActivity`: dispara quando uma atividade muda (o padrão nunca dispara).
    fn on_did_change_activity(&self) -> Event<()> {
        Event::none()
    }

    /// `onDidChangeActivity` com o alvo que o original carrega (`string | IAction`): o consumidor que filtra pelo
    /// identificador (os itens globais, `GlobalCompositeBar`) lê este. O padrão nunca dispara.
    fn on_did_change_activity_target(&self) -> Event<ActivityChangeTarget> {
        Event::none()
    }

    /// `getActivity(id)`: as atividades do identificador (o padrão é a lista vazia).
    fn get_activity(&self, _id: &str) -> Vec<IActivity> {
        Vec::new()
    }

    /// `getViewContainerActivities(viewContainerId)`: as atividades do contêiner de views (o padrão é a lista vazia).
    fn get_view_container_activities(&self, _view_container_id: &str) -> Vec<IActivity> {
        Vec::new()
    }

    /// `showViewActivity(viewId, badge)`: mostra o selo da view até o descarte do retorno.
    fn show_view_activity(&self, view_id: &str, badge: IActivity) -> DisposableRef;

    /// `showAccountsActivity(activity)`: mostra o selo do menu de contas até o descarte do retorno. Corpo padrão que lança
    /// `TypeError: showAccountsActivity is not a function` (a barra de atividades e o menu de contas ainda não foram portados,
    /// então só um serviço que os tenha o implementa).
    fn show_accounts_activity(&self, _activity: IActivity) -> DisposableRef {
        je_base::errors::throw(je_base::errors::Error::named("TypeError", "showAccountsActivity is not a function"))
    }
}

je_instantiation::service_id!(pub I_ACTIVITY_SERVICE: dyn IActivityService = "activityService");
