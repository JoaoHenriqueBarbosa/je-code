//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/typingSpeed.ts`: o [`TypingInterval`], que mede a
//! velocidade de digitação como a média de milissegundos entre toques de tecla.
//!
//! Notas de tradução:
//! - `Disposable` (classe base) é o `Rc<Disposable>` guardado em `base`; o estado mutável mora num `Rc<RefCell<..>>`
//!   compartilhado com o ouvinte de `onDidChangeContent`;
//! - `Date.now()` é [`je_base::runtime::date_now`] (milissegundos);
//! - `reason.metadata.isUndoing` e `isRedoing` são lidos como o texto `true` do metadado, a representação de
//!   booleanos de [`TextModelEditSource`];
//! - `IModelContentChangedEvent.changes[].text.length` conta unidades UTF-16, como o [`JsString`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::runtime::date_now;

use crate::text_model::TextModel;
use crate::text_model_edit_source::TextModelEditSource;
use crate::text_model_events::ModelContentChangedEvent;

/// `TypingSession`.
#[derive(Clone, Copy, Debug)]
struct TypingSession {
    start_time: i64,
    end_time: i64,
    /// Contagem efetiva de caracteres para o cálculo do intervalo de digitação.
    character_count: i64,
}

/// `TypingIntervalResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypingIntervalResult {
    /// Média de milissegundos entre toques de tecla.
    pub average_interval: i64,
    /// Número de caracteres envolvidos no cálculo.
    pub character_count: i64,
}

/// Máximo de intervalo (3 segundos) entre toques de tecla dentro de uma sessão.
const MAX_SESSION_GAP_MS: i64 = 3_000;
/// Duração mínima de sessão a considerar.
const MIN_SESSION_DURATION_MS: i64 = 1_000;
/// Mantém as últimas 50 sessões para o cálculo.
const SESSION_HISTORY_LIMIT: usize = 50;
/// Janela de 5 minutos para o cálculo da velocidade.
const TYPING_SPEED_WINDOW_MS: i64 = 300_000;
/// Mínimo de caracteres para uma velocidade confiável.
const MIN_CHARS_FOR_RELIABLE_SPEED: i64 = 20;

#[derive(Default)]
struct State {
    typing_sessions: Vec<TypingSession>,
    current_session: Option<TypingSession>,
    last_change_time: i64,
    cached_typing_interval_result: Option<TypingIntervalResult>,
    cache_invalidated: bool,
}

impl State {
    fn new() -> Self {
        Self { cache_invalidated: true, ..Self::default() }
    }

    fn update_typing_speed(&mut self, change: &ModelContentChangedEvent) {
        let now = date_now() as i64;

        if !is_user_typing(change) {
            self.finalize_current_session();
            return;
        }

        // If too much time has passed since last change, start a new session
        if self.current_session.is_some() && (now - self.last_change_time) > MAX_SESSION_GAP_MS {
            self.finalize_current_session();
        }

        // Start new session if none exists
        let session = self.current_session.get_or_insert(TypingSession {
            start_time: now,
            end_time: now,
            character_count: 0,
        });

        // Update current session
        session.end_time = now;
        session.character_count += actual_character_count(change);

        self.last_change_time = now;
        self.cache_invalidated = true;
    }

    fn finalize_current_session(&mut self) {
        let Some(session) = self.current_session.take() else {
            return;
        };

        let session_duration = session.end_time - session.start_time;

        // Only keep sessions that meet minimum duration and have actual content
        if session_duration >= MIN_SESSION_DURATION_MS && session.character_count > 0 {
            self.typing_sessions.push(session);

            // Limit session history
            if self.typing_sessions.len() > SESSION_HISTORY_LIMIT {
                self.typing_sessions.remove(0);
            }
        }
    }

    fn calculate_typing_interval(&self) -> TypingIntervalResult {
        // Finalize current session for calculation
        if let Some(current) = self.current_session {
            let session_duration = current.end_time - current.start_time;
            if session_duration >= MIN_SESSION_DURATION_MS && current.character_count > 0 {
                let mut all_sessions = self.typing_sessions.clone();
                all_sessions.push(current);
                return calculate_speed_from_sessions(&all_sessions);
            }
        }

        calculate_speed_from_sessions(&self.typing_sessions)
    }
}

/// `_getActualCharacterCount`: os caracteres acrescentados ou removidos (o maior dos dois por mudança).
fn actual_character_count(change: &ModelContentChangedEvent) -> i64 {
    change.changes.iter().map(|c| c.text.len().max(c.range_length) as i64).sum()
}

/// `_isUserTyping`.
fn is_user_typing(change: &ModelContentChangedEvent) -> bool {
    // If no detailed reasons, it is not user typing
    if change.detailed_reasons.is_empty() {
        return false;
    }

    // Check if any of the reasons indicate actual user typing
    change.detailed_reasons.iter().any(is_user_typing_reason)
}

/// `_isUserTypingReason`.
fn is_user_typing_reason(reason: &TextModelEditSource) -> bool {
    // Handle undo/redo - not considered user typing
    if reason.metadata_value("isUndoing") == Some("true") || reason.metadata_value("isRedoing") == Some("true") {
        return false;
    }

    // Handle different source types
    match reason.metadata_value("source") {
        Some("cursor") => {
            // Direct user input via cursor
            matches!(reason.metadata_value("kind"), Some("type" | "compositionType" | "compositionEnd"))
        }
        // All other sources (paste, suggestions, code actions, etc.) are not user typing
        _ => false,
    }
}

/// `_calculateSpeedFromSessions`.
fn calculate_speed_from_sessions(sessions: &[TypingSession]) -> TypingIntervalResult {
    if sessions.is_empty() {
        return TypingIntervalResult { average_interval: 0, character_count: 0 };
    }

    // Sort sessions by recency (most recent first) to ensure we get the most recent sessions
    let mut sorted_sessions = sessions.to_vec();
    sorted_sessions.sort_by(|a, b| b.end_time.cmp(&a.end_time));

    // First, try the standard window
    let cutoff_time = date_now() as i64 - TYPING_SPEED_WINDOW_MS;
    let mut recent_sessions: Vec<TypingSession> =
        sorted_sessions.iter().copied().filter(|session| session.end_time > cutoff_time).collect();
    let older_sessions = sorted_sessions.split_off(recent_sessions.len());

    let mut total_chars: i64 = recent_sessions.iter().map(|session| session.character_count).sum();

    // If we don't have enough characters in the standard window, expand to include older sessions
    for older in &older_sessions {
        if total_chars >= MIN_CHARS_FOR_RELIABLE_SPEED {
            break;
        }
        recent_sessions.push(*older);
        total_chars += older.character_count;
    }

    let total_time: i64 = recent_sessions.iter().map(|session| session.end_time - session.start_time).sum();
    if total_time == 0 || total_chars <= 1 {
        return TypingIntervalResult { average_interval: 0, character_count: total_chars };
    }

    // Calculate average milliseconds between keystrokes
    let keystroke_intervals = (total_chars - 1).max(1);
    let avg_ms_between_keystrokes = total_time as f64 / keystroke_intervals as f64;

    TypingIntervalResult {
        average_interval: avg_ms_between_keystrokes.round() as i64,
        character_count: total_chars,
    }
}

/// `TypingInterval`: a velocidade de digitação como a média de milissegundos entre toques de tecla.
/// Valores maiores indicam digitação mais lenta.
pub struct TypingInterval {
    base: Rc<Disposable>,
    state: Rc<RefCell<State>>,
}

impl TypingInterval {
    pub fn new(text_model: &Rc<TextModel>) -> Rc<Self> {
        let base = Disposable::new();
        let state = Rc::new(RefCell::new(State::new()));

        let listener_state = Rc::clone(&state);
        base.register(text_model.on_did_change_content().listen(move |event| {
            listener_state.borrow_mut().update_typing_speed(event);
        }));

        Rc::new(Self { base, state })
    }

    /// `getTypingInterval()`: o intervalo médio entre toques e o número de caracteres usados no cálculo, ou
    /// `{ averageInterval: 0, characterCount: 0 }` se não há dados de digitação.
    pub fn get_typing_interval(&self) -> TypingIntervalResult {
        let mut state = self.state.borrow_mut();
        match state.cached_typing_interval_result {
            Some(result) if !state.cache_invalidated => result,
            _ => {
                let result = state.calculate_typing_interval();
                state.cached_typing_interval_result = Some(result);
                state.cache_invalidated = false;
                result
            }
        }
    }

    /// `reset()`: apaga todos os dados de velocidade de digitação.
    pub fn reset(&self) {
        let mut state = self.state.borrow_mut();
        state.typing_sessions.clear();
        state.current_session = None;
        state.last_change_time = 0;
        state.cached_typing_interval_result = None;
        state.cache_invalidated = true;
    }
}

impl IDisposable for TypingInterval {
    fn dispose(&self) {
        self.state.borrow_mut().finalize_current_session();
        self.base.dispose();
    }
}
