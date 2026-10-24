//! Porte da parte lógica da pilha de chamadas de depuração: de `browser/callStackView.ts` (`getContext`,
//! `getContextForContributedActions`, `getSpecificSourceName`, `getThreadContextOverlay`, `getSessionContextOverlay`), de
//! `browser/callStackEditorContribution.ts` (`createDecorationsForStackFrame` e as opções de decoração) e de
//! `browser/debugService.ts` (`getStackFrameThreadAndSessionToFocus`).
//!
//! Notas de tradução:
//! - `CallStackItem` (`IDebugSession | IThread | IStackFrame | ThreadAndSessionIds | ...`, onde as classes se distinguem com
//!   `instanceof`) é o enum [`CallStackItem`].
//! - Os valores do overlay de contexto (`ContextKeyValue`) são [`Dynamic`].
//! - A árvore, os renderizadores e o editor (`CallStackView`, `CallStackEditorContribution`) NÃO foram portados: são UI.
//! - `ThemeIcon.asClassName(debugStackframe)` usa o id `debug-stackframe` do codicon registrado em `debugIcons.ts`.
//! - `ICallStackItemContext.frameLocation` guarda o intervalo e o `source.raw` (o objeto do protocolo).

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::strings::common_suffix_length;
use je_base::themables::ThemeIcon;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;
use je_editor::model::{
    GlyphMarginLane, InjectedTextOptions, ModelDecorationGlyphMarginInit, ModelDecorationOptionsInit, ModelDecorationOverviewRulerInit,
    ModelDeltaDecoration, OverviewRulerLane, TrackedRangeStickiness, DecorationColor,
};
use je_editor::range::Range;
use je_editor::text_model::ModelDecorationOptions;

use super::debug_model::{StackFrame, Thread, has_multiple_stack_frames};
use super::debug_protocol::Source as ProtocolSource;
use super::debug_types::{IDebugSessionFull, State};
use super::debug_utils::is_session_attach;

pub const CONTEXT_CALLSTACK_ITEM_TYPE: &str = "callStackItemType";
pub const CONTEXT_CALLSTACK_SESSION_IS_ATTACH: &str = "callStackSessionIsAttach";
pub const CONTEXT_CALLSTACK_ITEM_STOPPED: &str = "callStackItemStopped";
pub const CONTEXT_CALLSTACK_SESSION_HAS_ONE_THREAD: &str = "callStackSessionHasOneThread";
pub const CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES: &str = super::debug_view_model::CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES_KEY;

/// `CallStackItem`, só os três tipos que o contexto distingue.
#[derive(Clone)]
pub enum CallStackItem {
    Session(Rc<dyn IDebugSessionFull>),
    Thread(Rc<Thread>),
    StackFrame(Rc<StackFrame>),
}

/// `ICallStackItemContext`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallStackItemContext {
    pub session_id: String,
    pub thread_id: Option<String>,
    pub frame_id: Option<String>,
    pub frame_name: Option<String>,
    pub frame_location: Option<(Option<Range>, ProtocolSource)>,
}

fn get_session_context(element: &Rc<dyn IDebugSessionFull>) -> CallStackItemContext {
    CallStackItemContext { session_id: element.get_id(), ..CallStackItemContext::default() }
}

fn get_thread_context(element: &Rc<Thread>) -> CallStackItemContext {
    CallStackItemContext { thread_id: Some(element.get_id()), ..get_session_context(&element.session) }
}

fn get_stack_frame_context(element: &Rc<StackFrame>) -> CallStackItemContext {
    CallStackItemContext {
        frame_id: Some(element.get_id()),
        frame_name: Some(element.name.clone()),
        frame_location: Some((element.range, element.source.raw.borrow().clone())),
        ..get_thread_context(&element.thread)
    }
}

/// `getContext(element)`.
pub fn get_context(element: Option<&CallStackItem>) -> Option<CallStackItemContext> {
    match element? {
        CallStackItem::StackFrame(frame) => Some(get_stack_frame_context(frame)),
        CallStackItem::Thread(thread) => Some(get_thread_context(thread)),
        CallStackItem::Session(session) => Some(get_session_context(session)),
    }
}

/// O `string | number` de `getContextForContributedActions`.
#[derive(Clone, Debug, PartialEq)]
pub enum ContributedActionsContext {
    Text(String),
    Number(i64),
}

/// `getContextForContributedActions(element)`: extensions depend on this context, should not be changed even though it is
/// not fully deterministic.
pub fn get_context_for_contributed_actions(element: Option<&CallStackItem>) -> ContributedActionsContext {
    match element {
        Some(CallStackItem::StackFrame(frame)) => {
            if frame.source.in_memory() {
                let raw = frame.source.raw.borrow();
                if let Some(path) = raw.path.clone().filter(|p| !p.is_empty()) {
                    return ContributedActionsContext::Text(path);
                }
                if let Some(reference) = raw.source_reference.filter(|r| *r != 0) {
                    return ContributedActionsContext::Number(reference);
                }
                return ContributedActionsContext::Text(frame.source.name());
            }

            ContributedActionsContext::Text(frame.source.uri.to_string(false).to_string_lossy())
        }
        Some(CallStackItem::Thread(thread)) => ContributedActionsContext::Number(thread.thread_id),
        Some(CallStackItem::Session(session)) => ContributedActionsContext::Text(session.get_id()),
        None => ContributedActionsContext::Text(String::new()),
    }
}

/// `getSpecificSourceName(stackFrame)`.
pub fn get_specific_source_name(stack_frame: &Rc<StackFrame>) -> String {
    // To reduce flashing of the path name and the way we fetch stack frames
    // We need to compute the source name based on the other frames in the stale call stack
    let mut call_stack = stack_frame.thread.get_stale_call_stack();
    if call_stack.is_empty() {
        call_stack = stack_frame.thread.get_call_stack();
    }
    let other_sources: Vec<_> = call_stack.iter().map(|sf| sf.source.clone()).filter(|s| !Rc::ptr_eq(s, &stack_frame.source)).collect();
    let mut suffix_length = 0;
    let path: Vec<u16> = stack_frame.source.uri.path().to_string_lossy().encode_utf16().collect();
    for s in &other_sources {
        if s.name() == stack_frame.source.name() {
            let other: Vec<u16> = s.uri.path().to_string_lossy().encode_utf16().collect();
            suffix_length = suffix_length.max(common_suffix_length(&path, &other));
        }
    }
    if suffix_length == 0 {
        return stack_frame.source.name();
    }

    let posix = PathModule::posix(&ProcessEnvironment);
    let sep: Vec<u16> = posix.sep().encode_utf16().collect();
    let from_index = path.len() as i64 - suffix_length as i64 - 1;
    let from = if from_index < 0 {
        0
    } else {
        je_base::js_string::last_index_of(&path, &sep, from_index as usize).unwrap_or(0)
    };
    format!("{}{}", if from > 0 { "..." } else { "" }, String::from_utf16_lossy(&path[from..]))
}

/// `getSessionContextOverlay(session)`.
pub fn get_session_context_overlay(session: &Rc<dyn IDebugSessionFull>) -> Vec<(String, Dynamic)> {
    let threads = session.get_all_threads();
    let single = if threads.len() == 1 { Some(threads[0].as_ref()) } else { None };
    let as_session: Rc<dyn super::debug::IDebugSession> = session.clone();
    vec![
        (CONTEXT_CALLSTACK_ITEM_TYPE.to_owned(), Dynamic::from("session")),
        (CONTEXT_CALLSTACK_SESSION_IS_ATTACH.to_owned(), Dynamic::Bool(is_session_attach(&as_session))),
        (CONTEXT_CALLSTACK_ITEM_STOPPED.to_owned(), Dynamic::Bool(session.state() == State::Stopped)),
        (CONTEXT_CALLSTACK_SESSION_HAS_ONE_THREAD.to_owned(), Dynamic::Bool(threads.len() == 1)),
        // The session row can target a thread only when the session has exactly one.
        (CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES.to_owned(), Dynamic::Bool(has_multiple_stack_frames(single))),
    ]
}

/// `getThreadContextOverlay(thread)`.
pub fn get_thread_context_overlay(thread: &Thread) -> Vec<(String, Dynamic)> {
    vec![
        (CONTEXT_CALLSTACK_ITEM_TYPE.to_owned(), Dynamic::from("thread")),
        (CONTEXT_CALLSTACK_ITEM_STOPPED.to_owned(), Dynamic::Bool(thread.stopped.get())),
        (CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES.to_owned(), Dynamic::Bool(has_multiple_stack_frames(Some(thread)))),
    ]
}

// ---- decorações do editor

/// `debugStackframe` (`ThemeIcon.asClassName`).
pub fn debug_stackframe_class_name() -> String {
    ThemeIcon::from_id("debug-stackframe").as_class_name()
}

/// `debugStackframeFocused` (`ThemeIcon.asClassName`).
pub fn debug_stackframe_focused_class_name() -> String {
    ThemeIcon::from_id("debug-stackframe-focused").as_class_name()
}

const STICKINESS: TrackedRangeStickiness = TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges;

fn margin_decoration(description: &str, icon_class_name: String, color: &str) -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: description.to_owned(),
        glyph_margin_class_name: Some(icon_class_name),
        glyph_margin: Some(ModelDecorationGlyphMarginInit { position: GlyphMarginLane::Right, persist_lane: None }),
        z_index: 9999,
        stickiness: Some(STICKINESS),
        overview_ruler: Some(ModelDecorationOverviewRulerInit {
            position: Some(OverviewRulerLane::Full),
            color: Some(DecorationColor::Theme(color.to_owned())),
            dark_color: None,
        }),
        ..ModelDecorationOptionsInit::default()
    })
}

/// `TOP_STACK_FRAME_MARGIN`: we need a separate decoration for glyph margin, since we do not want it on each line of a
/// multi line statement.
pub fn top_stack_frame_margin() -> Rc<ModelDecorationOptions> {
    margin_decoration("top-stack-frame-margin", debug_stackframe_class_name(), "editor.stackFrameHighlightBackground")
}

/// `FOCUSED_STACK_FRAME_MARGIN`.
pub fn focused_stack_frame_margin() -> Rc<ModelDecorationOptions> {
    margin_decoration("focused-stack-frame-margin", debug_stackframe_focused_class_name(), "editor.focusedStackFrameHighlightBackground")
}

fn whole_line_decoration(description: &str, class_name: &str) -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: description.to_owned(),
        is_whole_line: true,
        class_name: Some(class_name.to_owned()),
        stickiness: Some(STICKINESS),
        ..ModelDecorationOptionsInit::default()
    })
}

/// `TOP_STACK_FRAME_DECORATION`.
pub fn top_stack_frame_decoration() -> Rc<ModelDecorationOptions> {
    whole_line_decoration("top-stack-frame-decoration", "debug-top-stack-frame-line")
}

/// `FOCUSED_STACK_FRAME_DECORATION`.
pub fn focused_stack_frame_decoration() -> Rc<ModelDecorationOptions> {
    whole_line_decoration("focused-stack-frame-decoration", "debug-focused-stack-frame-line")
}

/// `makeStackFrameColumnDecoration(noCharactersBefore)`.
pub fn make_stack_frame_column_decoration(no_characters_before: bool) -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "top-stack-frame-inline-decoration".to_owned(),
        before: Some(InjectedTextOptions {
            content: "\u{EB8B}".to_owned(),
            inline_class_name: Some(
                if no_characters_before { "debug-top-stack-frame-column start-of-line" } else { "debug-top-stack-frame-column" }.to_owned(),
            ),
            inline_class_name_affects_letter_spacing: true,
            ..InjectedTextOptions::default()
        }),
        ..ModelDecorationOptionsInit::default()
    })
}

/// `createDecorationsForStackFrame(stackFrame, isFocusedSession, noCharactersBefore)`.
pub fn create_decorations_for_stack_frame(stack_frame: &Rc<StackFrame>, is_focused_session: bool, no_characters_before: bool) -> Vec<ModelDeltaDecoration> {
    // only show decorations for the currently focused thread.
    let mut result = Vec::new();
    let frame_range = stack_frame.range.expect("o quadro de pilha tem intervalo");
    let column_until_eol_range = Range::new(frame_range.start_line_number, frame_range.start_column, frame_range.start_line_number, MAX_SAFE_SMALL_INTEGER);
    let range = Range::new(frame_range.start_line_number, frame_range.start_column, frame_range.start_line_number, frame_range.start_column + 1);

    // compute how to decorate the editor. Different decorations are used if this is a top stack frame, focused stack frame,
    // an exception or a stack frame that did not change the line number (we only decorate the columns, not the whole line).
    let top_stack_frame = stack_frame.thread.get_top_stack_frame();
    if top_stack_frame.as_ref().is_some_and(|top| stack_frame.get_id() == top.get_id()) {
        if is_focused_session {
            result.push(ModelDeltaDecoration { options: top_stack_frame_margin(), range });
        }

        result.push(ModelDeltaDecoration { options: top_stack_frame_decoration(), range: column_until_eol_range });

        if frame_range.start_column > 1 {
            result.push(ModelDeltaDecoration { options: make_stack_frame_column_decoration(no_characters_before), range: column_until_eol_range });
        }
    } else {
        if is_focused_session {
            result.push(ModelDeltaDecoration { options: focused_stack_frame_margin(), range });
        }

        result.push(ModelDeltaDecoration { options: focused_stack_frame_decoration(), range: column_until_eol_range });
    }

    result
}

// ---- foco

/// O `{ stackFrame, thread, session }` de `getStackFrameThreadAndSessionToFocus`.
#[derive(Clone)]
pub struct StackFrameThreadAndSession {
    pub stack_frame: Option<Rc<StackFrame>>,
    pub thread: Option<Rc<Thread>>,
    pub session: Option<Rc<dyn IDebugSessionFull>>,
}

/// `getStackFrameThreadAndSessionToFocus(model, stackFrame, thread, session, avoidSession)`.
pub fn get_stack_frame_thread_and_session_to_focus(
    model: &super::debug_model::DebugModel,
    mut stack_frame: Option<Rc<StackFrame>>,
    mut thread: Option<Rc<Thread>>,
    mut session: Option<Rc<dyn IDebugSessionFull>>,
    avoid_session: Option<Rc<dyn IDebugSessionFull>>,
) -> StackFrameThreadAndSession {
    if session.is_none() {
        if stack_frame.is_some() || thread.is_some() {
            session = Some(match &stack_frame {
                Some(frame) => frame.thread.session.clone(),
                None => thread.as_ref().expect("a thread").session.clone(),
            });
        } else {
            let sessions = model.get_sessions(false);
            let stopped_session = sessions.iter().find(|s| s.state() == State::Stopped).cloned();
            let same = |a: &Rc<dyn IDebugSessionFull>, b: &Rc<dyn IDebugSessionFull>| std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b));
            session = stopped_session
                .or_else(|| {
                    sessions
                        .iter()
                        .find(|s| {
                            avoid_session.as_ref().is_none_or(|avoid| {
                                !same(s, avoid)
                                    && !avoid.parent_session().is_some_and(|parent| std::ptr::addr_eq(Rc::as_ptr(&parent), Rc::as_ptr(*s)))
                            })
                        })
                        .cloned()
                })
                .or_else(|| sessions.first().cloned());
        }
    }

    if thread.is_none() {
        if let Some(frame) = &stack_frame {
            thread = Some(frame.thread.clone());
        } else {
            let threads = session.as_ref().map(|s| s.get_all_threads());
            let stopped_thread = threads.as_ref().and_then(|threads| threads.iter().find(|t| t.stopped.get()).cloned());
            thread = stopped_thread.or_else(|| threads.as_ref().and_then(|threads| threads.first().cloned()));
        }
    }

    if stack_frame.is_none()
        && let Some(thread) = &thread
    {
        stack_frame = thread.get_top_stack_frame();
    }

    StackFrameThreadAndSession { session, thread, stack_frame }
}
