//! Porte de `src/vs/workbench/services/userAttention/common/userAttentionService.ts`: o `IUserAttentionService`,
//! o serviço que acompanha se o usuário está prestando atenção ao VS Code.
//!
//! Divergências:
//! - `IObservable<boolean>` é [`IObservable<bool>`] e `totalFocusTimeMs` (`number`) é `u64`, como o
//!   `TotalTrueTimeObservable` do porte.
//! - `fireAfterGivenFocusTimePassed(focusTimeMs, callback)` recebe o `callback` como `Box<dyn Fn()>` (o trait
//!   precisa ser usado como `dyn`, então o método não pode ser genérico).
//! - `createDecorator('userAttentionService')` é [`I_USER_ATTENTION_SERVICE`], com o proxy
//!   [`UserAttentionServiceProxy`] que a instanciação atrasada (`InstantiationType.Delayed`) exige.

use je_base::lifecycle::DisposableRef;
use je_base::observable::IObservable;
use je_instantiation::{delayed_proxy, service_id};

/// `IUserAttentionService`.
///
/// Service that tracks whether the user is actively paying attention to VS Code.
///
/// This is determined by:
/// * VS Code window has focus
/// * User has performed some activity (keyboard/mouse) within the last minute
pub trait IUserAttentionService {
    fn is_vs_code_focused(&self) -> IObservable<bool>;

    /// Observable that is true when user activity was recently detected (within the last 500ms).
    /// This includes keyboard typing and mouse movements/clicks while VS Code is focused.
    /// The 500ms window prevents event spam from continuous mouse movement.
    fn is_user_active(&self) -> IObservable<bool>;

    /// Observable that indicates whether the user is actively paying attention to VS Code.
    /// This is true when:
    /// * VS Code has focus, AND
    /// * There was user activity within the last minute
    fn has_user_attention(&self) -> IObservable<bool>;

    /// The total time in milliseconds that the user has been paying attention to VS Code.
    fn total_focus_time_ms(&self) -> u64;

    /// Fires the callback after the user has accumulated the specified amount of focus time.
    /// Focus time is computed as the number of 1-minute blocks in which the user has attention
    /// (`has_user_attention` is true).
    ///
    /// - `focus_time_ms`: the accumulated focus time in milliseconds before the callback is fired.
    /// - `callback`: the callback to fire once the focus time has been accumulated.
    ///
    /// Returns a disposable that cancels the callback when disposed.
    fn fire_after_given_focus_time_passed(&self, focus_time_ms: u64, callback: Box<dyn Fn()>) -> DisposableRef;
}

delayed_proxy!(pub UserAttentionServiceProxy for dyn IUserAttentionService {
    methods {
        fn is_vs_code_focused(&self) -> IObservable<bool>;
        fn is_user_active(&self) -> IObservable<bool>;
        fn has_user_attention(&self) -> IObservable<bool>;
        fn total_focus_time_ms(&self) -> u64;
        fn fire_after_given_focus_time_passed(&self, focus_time_ms: u64, callback: Box<dyn Fn()>) -> DisposableRef;
    }
    events {}
});

service_id!(pub I_USER_ATTENTION_SERVICE: dyn IUserAttentionService = "userAttentionService", proxy UserAttentionServiceProxy);
