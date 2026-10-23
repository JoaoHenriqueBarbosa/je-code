//! Porte de `src/vs/platform/terminal/common/terminalRecorder.ts`, com os tipos que ele importa:
//! `ReplayEntry` (de `terminalProcess.ts`) e `IPtyHostProcessReplayEvent` (de
//! `capabilities/capabilities.ts`, que o porte de [`super::capabilities`] deixou de fora justamente por
//! depender de `ReplayEntry`).
//!
//! Divergências:
//! - `data` é [`JsString`] (UTF-16): o `substr` do original corta por unidade de código e pode partir um par
//!   substituto, e o tamanho que conta para o limite de 10 MB é `string.length`, em unidades UTF-16.
//! - `number` é `f64` (colunas e linhas), como no resto do porte do terminal.
//! - `generateReplayEventSync` muta as entradas (junta os pedaços de `data`), então recebe `&mut self`;
//!   `generateReplayEvent` (`async`) devolve uma [`Promise`] já resolvida.
//! - `IPtyHostProcessReplayEvent` não é `Debug` nem `PartialEq` porque o modelo serializado do prompt é um
//!   `Rc<dyn ...>`; só o `events` é comparável.

use je_base::JsString;
use je_base::promise::Promise;

use super::capabilities::capabilities::ISerializedCommandDetectionCapability;

const MAX_RECORDER_DATA_SIZE: usize = 10 * 1024 * 1024; // 10MB

struct RecorderEntry {
    cols: f64,
    rows: f64,
    data: Vec<JsString>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayEntry {
    pub cols: f64,
    pub rows: f64,
    pub data: JsString,
}

#[derive(Clone)]
pub struct IPtyHostProcessReplayEvent {
    pub events: Vec<ReplayEntry>,
    pub commands: ISerializedCommandDetectionCapability,
}

pub struct IRemoteTerminalProcessReplayEvent {
    pub events: Vec<ReplayEntry>,
}

pub struct TerminalRecorder {
    entries: Vec<RecorderEntry>,
    total_data_length: usize,
}

impl TerminalRecorder {
    pub fn new(cols: f64, rows: f64) -> Self {
        Self { entries: vec![RecorderEntry { cols, rows, data: Vec::new() }], total_data_length: 0 }
    }

    pub fn handle_resize(&mut self, cols: f64, rows: f64) {
        if let Some(last_entry) = self.entries.last() {
            if last_entry.data.is_empty() {
                // last entry is just a resize, so just remove it
                self.entries.pop();
            }
        }

        if let Some(last_entry) = self.entries.last_mut() {
            if last_entry.cols == cols && last_entry.rows == rows {
                // nothing changed
                return;
            }
            if last_entry.cols == 0.0 && last_entry.rows == 0.0 {
                // we finally received a good size!
                last_entry.cols = cols;
                last_entry.rows = rows;
                return;
            }
        }

        self.entries.push(RecorderEntry { cols, rows, data: Vec::new() });
    }

    pub fn handle_data(&mut self, data: JsString) {
        let length = data.len();
        let last_entry = self.entries.last_mut().expect("o gravador sempre tem uma entrada");
        last_entry.data.push(data);

        self.total_data_length += length;
        while self.total_data_length > MAX_RECORDER_DATA_SIZE {
            let first_entry = &mut self.entries[0];
            let remaining_to_delete = self.total_data_length - MAX_RECORDER_DATA_SIZE;
            let first_length = first_entry.data[0].len();
            if remaining_to_delete >= first_length {
                // the first data piece must be deleted
                self.total_data_length -= first_length;
                first_entry.data.remove(0);
                if first_entry.data.is_empty() {
                    // the first entry must be deleted
                    self.entries.remove(0);
                }
            } else {
                // the first data piece must be partially deleted
                first_entry.data[0] = first_entry.data[0].slice(remaining_to_delete, first_length);
                self.total_data_length -= remaining_to_delete;
            }
        }
    }

    pub fn generate_replay_event_sync(&mut self) -> IPtyHostProcessReplayEvent {
        // normalize entries to one element per data array
        for entry in &mut self.entries {
            if !entry.data.is_empty() {
                let mut joined = JsString::new();
                for piece in &entry.data {
                    joined.push_units(piece.units());
                }
                entry.data = vec![joined];
            }
        }
        IPtyHostProcessReplayEvent {
            events: self
                .entries
                .iter()
                .map(|entry| ReplayEntry { cols: entry.cols, rows: entry.rows, data: entry.data.first().cloned().unwrap_or_default() })
                .collect(),
            // No command restoration is needed when relaunching terminals
            commands: ISerializedCommandDetectionCapability {
                is_windows_pty: false,
                has_rich_command_detection: false,
                commands: Vec::new(),
                prompt_input_model: None,
            },
        }
    }

    pub fn generate_replay_event(&mut self) -> Promise<IPtyHostProcessReplayEvent> {
        Promise::resolved(self.generate_replay_event_sync())
    }
}
