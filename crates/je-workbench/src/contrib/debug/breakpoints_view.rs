//! Porte da parte lógica de `src/vs/workbench/contrib/debug/browser/breakpointsView.ts`: `getExpandedBodySize` e
//! `getBreakpointMessageAndIcon` (a mensagem e o ícone de cada tipo de breakpoint), com os ícones de
//! `browser/debugIcons.ts` que ela escolhe.
//!
//! Notas de tradução:
//! - `BreakpointItem` (`IBreakpoint | IFunctionBreakpoint | IDataBreakpoint | IExceptionBreakpoint | IInstructionBreakpoint`,
//!   onde as classes se distinguem com `instanceof`) é o enum [`BreakpointElement`](super::debug_model::BreakpointElement).
//! - O `ThemeIcon` do resultado é o id do ícone (`icon.id`), em [`ThemeIcon`].
//! - A árvore, os renderizadores, as ações e os menus do arquivo (`BreakpointsView`, `registerAction2`...) NÃO foram portados:
//!   são UI.

use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_platform::label::label::{ILabelService, UriLabelOptions};

use super::debug_model::{BreakpointElement, DebugModel};
use super::debug_types::State;

/// O `{ message?, icon, showAdapterUnverifiedMessage? }` que `getBreakpointMessageAndIcon` devolve.
#[derive(Clone, Debug, PartialEq)]
pub struct BreakpointMessageAndIcon {
    pub message: Option<String>,
    pub icon: ThemeIcon,
    pub show_adapter_unverified_message: bool,
}

impl BreakpointMessageAndIcon {
    fn new(icon: &str, message: Option<String>) -> Self {
        Self { message, icon: ThemeIcon::from_id(icon), show_adapter_unverified_message: false }
    }
}

/// Os ícones de um tipo de breakpoint (`icons.breakpoint`, `icons.functionBreakpoint`...): `regular`, `disabled`, `unverified`.
struct BreakpointTypeIcons {
    regular: &'static str,
    disabled: &'static str,
    unverified: &'static str,
}

const BREAKPOINT: BreakpointTypeIcons =
    BreakpointTypeIcons { regular: "debug-breakpoint", disabled: "debug-breakpoint-disabled", unverified: "debug-breakpoint-unverified" };
const BREAKPOINT_PENDING: &str = "debug-breakpoint-pending";
const FUNCTION_BREAKPOINT: BreakpointTypeIcons = BreakpointTypeIcons {
    regular: "debug-breakpoint-function",
    disabled: "debug-breakpoint-function-disabled",
    unverified: "debug-breakpoint-function-unverified",
};
const CONDITIONAL_BREAKPOINT: &str = "debug-breakpoint-conditional";
const DATA_BREAKPOINT: BreakpointTypeIcons =
    BreakpointTypeIcons { regular: "debug-breakpoint-data", disabled: "debug-breakpoint-data-disabled", unverified: "debug-breakpoint-data-unverified" };
const LOG_BREAKPOINT: BreakpointTypeIcons =
    BreakpointTypeIcons { regular: "debug-breakpoint-log", disabled: "debug-breakpoint-log-disabled", unverified: "debug-breakpoint-log-unverified" };
const DEBUG_BREAKPOINT_UNSUPPORTED: &str = "debug-breakpoint-unsupported";

/// `getExpandedBodySize(model, sessionId, countLimit)`.
pub fn get_expanded_body_size(model: &DebugModel, session_id: Option<&str>, count_limit: usize) -> usize {
    let length = model.get_breakpoints(None).len()
        + model.get_exception_breakpoints_for_session(session_id).len()
        + model.get_function_breakpoints().len()
        + model.get_data_breakpoints().len()
        + model.get_instruction_breakpoints().len();
    count_limit.min(length) * 22
}

/// `getBreakpointMessageAndIcon(state, breakpointsActivated, breakpoint, labelService, debugModel)`.
pub fn get_breakpoint_message_and_icon(
    state: State,
    breakpoints_activated: bool,
    breakpoint: &BreakpointElement,
    label_service: &dyn ILabelService,
    debug_model: &DebugModel,
) -> BreakpointMessageAndIcon {
    let debug_active = state == State::Running || state == State::Stopped;

    let (base, enabled, log_message, condition, hit_condition, message, verified, supported) = match breakpoint {
        BreakpointElement::Breakpoint(bp) => (
            &bp.base,
            bp.enabled(),
            bp.log_message(),
            bp.condition(),
            bp.hit_condition(),
            bp.message(),
            bp.verified(),
            bp.supported(),
        ),
        BreakpointElement::Function(bp) => (
            &bp.base,
            bp.base.enabled(),
            bp.base.log_message.borrow().clone(),
            bp.base.condition.borrow().clone(),
            bp.base.hit_condition.borrow().clone(),
            bp.base.message(),
            bp.base.verified(),
            bp.supported(),
        ),
        BreakpointElement::Data(bp) => (
            &bp.base,
            bp.base.enabled(),
            bp.base.log_message.borrow().clone(),
            bp.base.condition.borrow().clone(),
            bp.base.hit_condition.borrow().clone(),
            bp.base.message(),
            bp.base.verified(),
            bp.supported(),
        ),
        BreakpointElement::Instruction(bp) => (
            &bp.base,
            bp.base.enabled(),
            bp.base.log_message.borrow().clone(),
            bp.base.condition.borrow().clone(),
            bp.base.hit_condition.borrow().clone(),
            bp.base.message(),
            bp.base.verified(),
            bp.supported(),
        ),
        BreakpointElement::Exception(bp) => (
            &bp.base,
            bp.base.enabled(),
            bp.base.log_message.borrow().clone(),
            bp.base.condition.borrow().clone(),
            bp.base.hit_condition.borrow().clone(),
            bp.base.message(),
            bp.base.verified(),
            bp.supported(),
        ),
    };
    let _ = base;
    let non_empty = |text: &Option<String>| text.clone().filter(|t| !t.is_empty());
    let log_message = non_empty(&log_message);
    let condition = non_empty(&condition);
    let hit_condition = non_empty(&hit_condition);
    let message = non_empty(&message);

    let breakpoint_icon = match breakpoint {
        BreakpointElement::Data(_) => &DATA_BREAKPOINT,
        BreakpointElement::Function(_) => &FUNCTION_BREAKPOINT,
        _ if log_message.is_some() => &LOG_BREAKPOINT,
        _ => &BREAKPOINT,
    };

    if !enabled || !breakpoints_activated {
        return BreakpointMessageAndIcon::new(
            breakpoint_icon.disabled,
            Some(if log_message.is_some() { localize("disabledLogpoint", "Disabled Logpoint", &[]) } else { localize("disabledBreakpoint", "Disabled Breakpoint", &[]) }),
        );
    }

    let append_message = |text: String| -> String {
        match &message {
            Some(message) => format!("{text}, {message}"),
            None => text,
        }
    };

    if debug_active && let BreakpointElement::Breakpoint(bp) = breakpoint && bp.pending() {
        return BreakpointMessageAndIcon::new(BREAKPOINT_PENDING, None);
    }

    if debug_active && !verified {
        return BreakpointMessageAndIcon {
            icon: ThemeIcon::from_id(breakpoint_icon.unverified),
            message: Some(message.clone().unwrap_or_else(|| {
                if log_message.is_some() { localize("unverifiedLogpoint", "Unverified Logpoint", &[]) } else { localize("unverifiedBreakpoint", "Unverified Breakpoint", &[]) }
            })),
            show_adapter_unverified_message: true,
        };
    }

    if let BreakpointElement::Data(_) = breakpoint {
        if !supported {
            return BreakpointMessageAndIcon::new(
                breakpoint_icon.unverified,
                Some(localize("dataBreakpointUnsupported", "Data breakpoints not supported by this debug type", &[])),
            );
        }

        return BreakpointMessageAndIcon::new(breakpoint_icon.regular, Some(message.clone().unwrap_or_else(|| localize("dataBreakpoint", "Data Breakpoint", &[]))));
    }

    if let BreakpointElement::Function(_) = breakpoint {
        if !supported {
            return BreakpointMessageAndIcon::new(
                breakpoint_icon.unverified,
                Some(localize("functionBreakpointUnsupported", "Function breakpoints not supported by this debug type", &[])),
            );
        }
        let mut messages: Vec<String> = Vec::new();
        messages.push(message.clone().unwrap_or_else(|| localize("functionBreakpoint", "Function Breakpoint", &[])));
        if let Some(condition) = &condition {
            messages.push(localize("expression", "Condition: {0}", &[condition]));
        }
        if let Some(hit_condition) = &hit_condition {
            messages.push(localize("hitCount", "Hit Count: {0}", &[hit_condition]));
        }

        return BreakpointMessageAndIcon::new(breakpoint_icon.regular, Some(append_message(messages.join("\n"))));
    }

    if let BreakpointElement::Instruction(bp) = breakpoint {
        if !supported {
            return BreakpointMessageAndIcon::new(
                breakpoint_icon.unverified,
                Some(localize("instructionBreakpointUnsupported", "Instruction breakpoints not supported by this debug type", &[])),
            );
        }
        let mut messages: Vec<String> = Vec::new();
        if let Some(message) = &message {
            messages.push(message.clone());
        } else if !bp.instruction_reference.is_empty() {
            messages.push(localize("instructionBreakpointAtAddress", "Instruction breakpoint at address {0}", &[&bp.instruction_reference]));
        } else {
            messages.push(localize("instructionBreakpoint", "Instruction breakpoint", &[]));
        }

        if let Some(hit_condition) = &hit_condition {
            messages.push(localize("hitCount", "Hit Count: {0}", &[hit_condition]));
        }

        return BreakpointMessageAndIcon::new(breakpoint_icon.regular, Some(append_message(messages.join("\n"))));
    }

    // can change this when all breakpoint supports dependent breakpoint condition
    let triggering_breakpoint = match breakpoint {
        BreakpointElement::Breakpoint(bp) => {
            let triggered_by = bp.triggered_by.borrow().clone().filter(|t| !t.is_empty());
            triggered_by.and_then(|triggered_by| debug_model.get_breakpoints(None).into_iter().find(|candidate| candidate.get_id() == triggered_by))
        }
        _ => None,
    };

    if log_message.is_some() || condition.is_some() || hit_condition.is_some() || triggering_breakpoint.is_some() {
        let mut messages: Vec<String> = Vec::new();
        let mut icon = if log_message.is_some() { LOG_BREAKPOINT.regular } else { CONDITIONAL_BREAKPOINT };
        if !supported {
            icon = DEBUG_BREAKPOINT_UNSUPPORTED;
            messages.push(localize("breakpointUnsupported", "Breakpoints of this type are not supported by the debugger", &[]));
        }

        if let Some(log_message) = &log_message {
            messages.push(localize("logMessage", "Log Message: {0}", &[log_message]));
        }
        if let Some(condition) = &condition {
            messages.push(localize("expression", "Condition: {0}", &[condition]));
        }
        if let Some(hit_condition) = &hit_condition {
            messages.push(localize("hitCount", "Hit Count: {0}", &[hit_condition]));
        }
        if let Some(triggering_breakpoint) = &triggering_breakpoint {
            let label = label_service.get_uri_label(&triggering_breakpoint.uri(), &UriLabelOptions { relative: Some(true), ..UriLabelOptions::default() });
            messages.push(localize("triggeredBy", "Hit after breakpoint: {0}", &[&format!("{}: {}", label, triggering_breakpoint.line_number())]));
        }

        return BreakpointMessageAndIcon::new(icon, Some(append_message(messages.join("\n"))));
    }

    let message = match (&message, breakpoint) {
        (Some(message), _) => message.clone(),
        (None, BreakpointElement::Breakpoint(bp)) => label_service.get_uri_label(&bp.uri(), &UriLabelOptions::default()),
        (None, _) => localize("breakpoint", "Breakpoint", &[]),
    };
    BreakpointMessageAndIcon::new(breakpoint_icon.regular, Some(message))
}
