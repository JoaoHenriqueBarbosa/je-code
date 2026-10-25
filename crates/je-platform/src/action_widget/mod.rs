//! Porte de `src/vs/platform/actionWidget`: o `ActionWidgetService` (`action_widget`), o contrato e o estado do `ActionList`
//! que ele usa (`action_list`, a fatia do `ActionList` completo continua dali), o movimento (`action_widget_motion`) e o
//! movimento do dropdown (`action_widget_dropdown`).

pub mod action_list;
pub mod action_widget;
pub mod action_widget_dropdown;
pub mod action_widget_motion;
pub mod tabbed_action_list_widget;
