//! Porte parcial de `src/vs/workbench/browser/parts/notifications/notificationsViewer.ts`: a altura global de uma
//! linha de notificação (`DEFAULT_NOTIFICATION_ROW_HEIGHT`, `COMPACT_NOTIFICATION_ROW_HEIGHT`,
//! `setNotificationRowHeight` e `onDidChangeNotificationRowHeight`), que o Modern UI troca.
//!
//! Recortes: `NotificationsListDelegate`, `NotificationRenderer` e o resto do arquivo são widgets do DOM e entram com
//! o centro de notificações.
//!
//! Divergências: o `let notificationRowHeight` do módulo é um estado por thread, como os outros globais do porte
//! (`set_global_pane_header_size`).

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};

/// Default height (px) of a single notification row.
pub const DEFAULT_NOTIFICATION_ROW_HEIGHT: f64 = 42.0;

/// Compact height (px) of a single notification row.
pub const COMPACT_NOTIFICATION_ROW_HEIGHT: f64 = 34.0;

thread_local! {
    static NOTIFICATION_ROW_HEIGHT: Cell<f64> = const { Cell::new(DEFAULT_NOTIFICATION_ROW_HEIGHT) };
    static ON_DID_CHANGE_NOTIFICATION_ROW_HEIGHT: Rc<Emitter<f64>> = Emitter::new();
}

/// `onDidChangeNotificationRowHeight`.
pub fn on_did_change_notification_row_height() -> Event<f64> {
    ON_DID_CHANGE_NOTIFICATION_ROW_HEIGHT.with(|emitter| emitter.event())
}

/// Current height (px) of a single notification row.
pub fn notification_row_height() -> f64 {
    NOTIFICATION_ROW_HEIGHT.with(Cell::get)
}

/// Overrides the height (px) of a single notification row and notifies listeners when it changes.
pub fn set_notification_row_height(height: f64) {
    if height != notification_row_height() {
        NOTIFICATION_ROW_HEIGHT.with(|current| current.set(height));
        ON_DID_CHANGE_NOTIFICATION_ROW_HEIGHT.with(|emitter| emitter.fire(height));
    }
}
