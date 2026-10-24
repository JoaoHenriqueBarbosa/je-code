//! Porte parcial de `src/vs/workbench/services/activity/common/activity.ts`: o contrato [`IActivityService`] que o
//! `CustomTreeView` usa para mostrar o selo de uma view (`showViewActivity`), com [`IActivity`], [`IBadge`] e o
//! [`NumberBadge`].
//!
//! Divergências:
//! - O resto da interface (`onDidChangeActivity`, `showViewContainerActivity`, `getViewContainerActivities`,
//!   `showAccountsActivity`, `showGlobalActivity`, `getActivity`) e os selos `IconBadge`, `ProgressBadge`, `WarningBadge` e
//!   `ErrorBadge` pertencem às partes de barra de atividade e de contas, que ainda não foram portadas.
//! - `IBadge.getColors(theme)` fica de fora: `IColorTheme` e `Color` são da camada de tema da interface. Só
//!   `getDescription()` entra.
//! - `IActivity` do original só tem o `badge`; o `priority` que o `CustomTreeView` põe no objeto literal é ignorado pelo
//!   serviço (a variável do original não passa pela checagem de propriedade extra), então não há campo aqui.

use std::rc::Rc;

use je_base::lifecycle::DisposableRef;

/// `IBadge`.
pub trait IBadge {
    fn get_description(&self) -> String;
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
}

/// `IActivityService`.
pub trait IActivityService {
    /// `showViewActivity(viewId, badge)`: mostra o selo da view até o descarte do retorno.
    fn show_view_activity(&self, view_id: &str, badge: IActivity) -> DisposableRef;
}

je_instantiation::service_id!(pub I_ACTIVITY_SERVICE: dyn IActivityService = "activityService");
