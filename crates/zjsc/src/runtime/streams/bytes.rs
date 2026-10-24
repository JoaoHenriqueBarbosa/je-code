//! A máquina de estados do `ReadableByteStreamController` (ReadableByteStreamInternals do WebKit/bun), sem motor JS:
//! fila de chunks, `pendingPullIntos`, `autoAllocateChunkSize`, `respond`, `respondWithNewView` e o preenchimento de
//! `read(view, { min })`.
//!
//! Os `ArrayBuffer` vivem numa arena própria ([`Buffers`]): o chunk de `enqueue` e a view de `read(view)` chegam já
//! transferidos (desanexados no lado JS) como bytes e voltam como [`BufferId`]. Toda operação que no C++ mexe em
//! promessas ou cria views devolve [`Action`]s, em ordem, para o lado JS aplicar (os membros `bc_*`, `bq_*` e `br_*`
//! de `mod.rs`, em `readable.rs`). Nenhum `RefCell` aqui: o chamador empresta o `ByteCore` por inteiro.
//!
//! Mensagens medidas no bun (linhas 426 a 479 do `streams_bun.tsv`) ficam em [`msg`].

use std::collections::VecDeque;

/// Identidade de um `ArrayBuffer` na arena.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct BufferId(pub(super) usize);

/// A arena de buffers: `None` é um buffer desanexado.
#[derive(Default)]
pub(super) struct Buffers(Vec<Option<Vec<u8>>>);

impl Buffers {
    pub(super) fn alloc(&mut self, data: Vec<u8>) -> BufferId {
        self.0.push(Some(data));
        BufferId(self.0.len() - 1)
    }

    pub(super) fn bytes(&self, id: BufferId) -> &[u8] {
        self.0[id.0].as_deref().unwrap_or(&[])
    }

    /// Grava `bytes` a partir de `offset` (o que passar do fim do buffer é descartado): é como o que o usuário escreveu
    /// na view do `byobRequest` volta para a arena.
    pub(super) fn write(&mut self, id: BufferId, offset: usize, bytes: &[u8]) {
        if let Some(target) = self.0[id.0].as_mut() {
            let end = (offset + bytes.len()).min(target.len());
            if offset < end {
                target[offset..end].copy_from_slice(&bytes[..end - offset]);
            }
        }
    }

    /// `TransferArrayBuffer` da view de `respondWithNewView`: o buffer do descritor passa a ter o conteúdo do novo.
    fn replace(&mut self, id: BufferId, data: Vec<u8>) {
        self.0[id.0] = Some(data);
    }

    fn copy_within_arena(&mut self, from: BufferId, from_offset: usize, to: BufferId, to_offset: usize, length: usize) {
        let chunk = self.bytes(from)[from_offset..from_offset + length].to_vec();
        if let Some(target) = self.0[to.0].as_mut() {
            target[to_offset..to_offset + length].copy_from_slice(&chunk);
        }
    }
}

/// As mensagens de erro medidas.
pub(super) mod msg {
    pub(in crate::runtime::streams) const VIEW_REQUIRED: &str = "ReadableStreamBYOBReader.prototype.read requires an ArrayBufferView";
    pub(in crate::runtime::streams) const VIEW_EMPTY: &str = "The view passed to read() must have a non-zero byteLength";
    pub(in crate::runtime::streams) const DETACHED: &str = "Buffer is already detached";
    pub(in crate::runtime::streams) const MIN_ZERO: &str = "The 'min' option must be greater than 0";
    pub(in crate::runtime::streams) const MIN_TOO_BIG: &str = "The 'min' option cannot be larger than the view passed to read()";
    pub(in crate::runtime::streams) const MIN_OPTIONS: &str = "ReadableStreamBYOBReader.prototype.read options must be an object";
    pub(in crate::runtime::streams) const RESPOND_ZERO: &str = "A readable byte stream's BYOB request cannot be responded to with 0 bytes written";
    pub(in crate::runtime::streams) const RESPOND_TOO_MANY: &str = "The number of bytes written exceeds the remaining length of the BYOB request's view";
    pub(in crate::runtime::streams) const INVALIDATED: &str = "This BYOB request has been invalidated";
    pub(in crate::runtime::streams) const NEW_VIEW_LENGTH: &str = "The argument 'view' must have the same buffer length as the BYOB request.";
    pub(in crate::runtime::streams) const NEW_VIEW_POSITION: &str = "The argument 'view' must match the BYOB request's current write position.";
    pub(in crate::runtime::streams) const CLOSED_NONZERO: &str = "bytes written must be 0 when the stream is closed";
}

/// Uma fatia de um buffer da arena (o `ReadableByteStreamQueueEntry`).
#[derive(Clone, Copy, Debug)]
pub(super) struct Chunk {
    pub(super) buffer: BufferId,
    pub(super) offset: usize,
    pub(super) length: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ReaderKind {
    None,
    Default,
    Byob,
}

/// O `PullIntoDescriptor`. `view_kind` é opaco: o lado JS o usa para refazer a view (Uint8Array, DataView...).
#[derive(Clone, Copy, Debug)]
pub(super) struct PullInto {
    pub(super) buffer: BufferId,
    pub(super) buffer_byte_length: usize,
    pub(super) byte_offset: usize,
    pub(super) byte_length: usize,
    pub(super) bytes_filled: usize,
    pub(super) minimum_fill: usize,
    pub(super) element_size: usize,
    pub(super) view_kind: u32,
    pub(super) reader: ReaderKind,
}

/// O que o lado JS aplica, em ordem.
#[derive(Debug)]
pub(super) enum Action {
    /// Resolve o pedido de leitura mais antigo do leitor padrão com um `Uint8Array` sobre o chunk.
    FulfillDefaultRead(Chunk),
    /// Resolve o pedido do leitor BYOB com a view refeita de `pull_into` (`elements` elementos já preenchidos).
    FulfillByobRead { pull_into: PullInto, elements: usize, done: bool },
    /// `ReadableStreamClose`.
    CloseStream,
    /// A view de `byobRequest` anterior foi invalidada.
    InvalidateByobRequest,
}

/// O estado do controlador de bytes.
pub(super) struct ByteCore {
    pub(super) buffers: Buffers,
    queue: VecDeque<Chunk>,
    queue_total: usize,
    pending: VecDeque<PullInto>,
    pub(super) high_water_mark: f64,
    pub(super) auto_allocate_chunk_size: Option<usize>,
    pub(super) started: bool,
    pub(super) close_requested: bool,
    pub(super) pulling: bool,
    pub(super) pull_again: bool,
    pub(super) closed: bool,
    pub(super) errored: bool,
}

impl ByteCore {
    pub(super) fn new(high_water_mark: f64, auto_allocate_chunk_size: Option<usize>) -> Self {
        ByteCore {
            buffers: Buffers::default(),
            queue: VecDeque::new(),
            queue_total: 0,
            pending: VecDeque::new(),
            high_water_mark,
            auto_allocate_chunk_size,
            started: false,
            close_requested: false,
            pulling: false,
            pull_again: false,
            closed: false,
            errored: false,
        }
    }

    /// `ReadableByteStreamControllerReleaseSteps`: o leitor saiu, os `pullInto` pendentes ficam sem leitor.
    pub(super) fn release_pull_into_readers(&mut self) {
        for descriptor in &mut self.pending {
            descriptor.reader = ReaderKind::None;
        }
    }

    /// `ReadableByteStreamControllerGetDesiredSize`: `None` com o stream em erro.
    pub(super) fn desired_size(&self) -> Option<f64> {
        if self.errored {
            None
        } else if self.closed {
            Some(0.0)
        } else {
            Some(self.high_water_mark - self.queue_total as f64)
        }
    }

    pub(super) fn can_close_or_enqueue(&self) -> bool {
        !self.close_requested && !self.closed && !self.errored
    }

    /// `ReadableByteStreamControllerShouldCallPull`; `read_requests` é o número de pedidos pendentes do leitor
    /// padrão, `reader` o tipo do leitor atual.
    pub(super) fn should_call_pull(&self, reader: ReaderKind, read_requests: usize) -> bool {
        if !self.can_close_or_enqueue() || !self.started {
            return false;
        }
        if reader == ReaderKind::Default && read_requests > 0 {
            return true;
        }
        if reader == ReaderKind::Byob && !self.pending.is_empty() {
            return true;
        }
        self.desired_size().is_some_and(|size| size > 0.0)
    }

    pub(super) fn clear_pending_pull_intos(&mut self) {
        self.pending.clear();
    }

    /// Quantos pedaços há na fila (cada um sai em uma leitura padrão).
    pub(super) fn queued_chunks(&self) -> usize {
        self.queue.len()
    }

    pub(super) fn reset_queue(&mut self) {
        self.queue.clear();
        self.queue_total = 0;
    }

    fn enqueue_chunk_to_queue(&mut self, chunk: Chunk) {
        self.queue_total += chunk.length;
        self.queue.push_back(chunk);
    }

    /// `ReadableByteStreamControllerEnqueue` depois das validações de JS: `data` já saiu do buffer do usuário
    /// (transferido). Devolve as ações a aplicar; o `pull` é decidido pelo chamador com [`Self::should_call_pull`].
    pub(super) fn enqueue(&mut self, data: Vec<u8>, reader: ReaderKind, read_requests: usize) -> Vec<Action> {
        let mut out = Vec::new();
        let length = data.len();
        let buffer = self.buffers.alloc(data);
        let chunk = Chunk { buffer, offset: 0, length };
        if !self.pending.is_empty() {
            out.push(Action::InvalidateByobRequest);
        }
        // Leitor padrão com pedidos de leitura: o chunk INTEIRO atende o pedido e o descritor do `autoAllocateChunkSize`
        // sai (`ShiftPendingPullInto`); medido no bun: `enqueue` de 5 bytes com `autoAllocateChunkSize` 3 entrega 5.
        if reader == ReaderKind::Default && read_requests > 0 {
            self.pending.pop_front();
            out.push(Action::FulfillDefaultRead(chunk));
            return out;
        }
        // Sem pedido pendente, com um `pullInto` (o do `autoAllocateChunkSize`) o chunk passa pela fila e preenche o descritor.
        if !self.pending.is_empty() && reader == ReaderKind::Default {
            self.enqueue_chunk_to_queue(chunk);
            self.process_pull_into_descriptors_using_queue(&mut out);
            return out;
        }
        match reader {
            ReaderKind::Default => {
                // Com pedidos pendentes a fila está vazia (invariante), então o chunk novo atende o próximo pedido.
                if read_requests == 0 {
                    self.enqueue_chunk_to_queue(chunk);
                } else {
                    out.push(Action::FulfillDefaultRead(chunk));
                }
            }
            ReaderKind::Byob => {
                self.enqueue_chunk_to_queue(chunk);
                self.process_pull_into_descriptors_using_queue(&mut out);
            }
            ReaderKind::None => self.enqueue_chunk_to_queue(chunk),
        }
        out
    }

    /// O trecho de `ReadableByteStreamControllerPullSteps` do leitor padrão com a fila não vazia: tira o primeiro chunk
    /// e, se a fila esvaziou com `close()` pedido, fecha o stream (`HandleQueueDrain`; a ação `CloseStream` sai antes
    /// de o chamador resolver a leitura com o chunk). `None` com a fila vazia.
    pub(super) fn dequeue_for_default_read(&mut self, out: &mut Vec<Action>) -> Option<Chunk> {
        let chunk = self.queue.pop_front()?;
        self.queue_total -= chunk.length;
        if self.queue_total == 0 && self.close_requested {
            self.closed = true;
            out.push(Action::CloseStream);
        }
        Some(chunk)
    }

    /// `ReadableByteStreamControllerProcessReadRequestsUsingQueue`: um pedido por chunk enfileirado.
    fn process_read_requests_using_queue(&mut self, mut read_requests: usize, out: &mut Vec<Action>) {
        while read_requests > 0 {
            let Some(chunk) = self.queue.pop_front() else { break };
            self.queue_total -= chunk.length;
            out.push(Action::FulfillDefaultRead(chunk));
            read_requests -= 1;
        }
    }

    /// `ReadableByteStreamControllerFillPullIntoDescriptorFromQueue`: devolve se o descritor ficou pronto.
    fn fill_from_queue(&mut self) -> bool {
        let Some(mut descriptor) = self.pending.front().copied() else { return false };
        let max_to_copy = self.queue_total.min(descriptor.byte_length - descriptor.bytes_filled);
        let filled_after = descriptor.bytes_filled + max_to_copy;
        let max_aligned = filled_after - filled_after % descriptor.element_size;
        let mut to_write = max_to_copy;
        let mut ready = false;
        if max_aligned >= descriptor.minimum_fill {
            to_write = max_aligned - descriptor.bytes_filled;
            ready = true;
        }
        while to_write > 0 {
            let Some(head) = self.queue.front().copied() else { break };
            let copy = to_write.min(head.length);
            self.buffers.copy_within_arena(head.buffer, head.offset, descriptor.buffer, descriptor.byte_offset + descriptor.bytes_filled, copy);
            if head.length == copy {
                self.queue.pop_front();
            } else if let Some(front) = self.queue.front_mut() {
                front.offset += copy;
                front.length -= copy;
            }
            self.queue_total -= copy;
            descriptor.bytes_filled += copy;
            to_write -= copy;
        }
        self.pending[0] = descriptor;
        ready
    }

    /// `ReadableByteStreamControllerProcessPullIntoDescriptorsUsingQueue`.
    pub(super) fn process_pull_into_descriptors_using_queue(&mut self, out: &mut Vec<Action>) {
        while !self.pending.is_empty() && self.queue_total > 0 {
            if self.fill_from_queue() {
                let descriptor = self.pending.pop_front().unwrap_or_else(|| unreachable_descriptor());
                self.commit(descriptor, out);
            } else {
                break;
            }
        }
    }

    /// `ReadableByteStreamControllerCommitPullIntoDescriptor`.
    fn commit(&mut self, descriptor: PullInto, out: &mut Vec<Action>) {
        let elements = descriptor.bytes_filled / descriptor.element_size;
        out.push(Action::FulfillByobRead { pull_into: descriptor, elements, done: self.closed });
    }

    /// `ReadableByteStreamControllerPullInto` para um `read(view, { min })` já validado: `data` é o conteúdo da view
    /// (o buffer foi transferido), `min_elements` o `min` em elementos.
    pub(super) fn pull_into(
        &mut self,
        data: Vec<u8>,
        byte_offset: usize,
        byte_length: usize,
        element_size: usize,
        min_elements: usize,
        view_kind: u32,
        out: &mut Vec<Action>,
    ) -> Result<(), &'static str> {
        let buffer_byte_length = data.len();
        let buffer = self.buffers.alloc(data);
        let descriptor = PullInto {
            buffer,
            buffer_byte_length,
            byte_offset,
            byte_length,
            bytes_filled: 0,
            minimum_fill: min_elements * element_size,
            element_size,
            view_kind,
            reader: ReaderKind::Byob,
        };
        if !self.pending.is_empty() {
            self.pending.push_back(descriptor);
            return Ok(());
        }
        if self.closed {
            let empty = PullInto { byte_length: 0, ..descriptor };
            out.push(Action::FulfillByobRead { pull_into: empty, elements: 0, done: true });
            return Ok(());
        }
        if self.queue_total > 0 {
            self.pending.push_back(descriptor);
            if self.fill_from_queue() {
                let ready = self.pending.pop_front().unwrap_or(descriptor);
                self.commit(ready, out);
                // `HandleQueueDrain`: a fila esvaziou com `close()` pedido, o stream fecha (a leitura que acabou de sair
                // continua com `done` falso, medido no bun; a seguinte já é a vazia com `done` verdadeiro).
                if self.queue_total == 0 && self.close_requested {
                    self.closed = true;
                    out.push(Action::CloseStream);
                }
                return Ok(());
            }
            if self.close_requested {
                return Err("close requested");
            }
            return Ok(());
        }
        self.pending.push_back(descriptor);
        Ok(())
    }

    /// `ReadableByteStreamControllerClose`: com `queue_total > 0` só marca o pedido; com fila vazia fecha. Devolve
    /// `Err` quando o primeiro descritor está parcialmente preenchido (o stream vira erro no lado JS).
    pub(super) fn close(&mut self, out: &mut Vec<Action>) -> Result<(), &'static str> {
        if !self.can_close_or_enqueue() {
            return Ok(());
        }
        if self.queue_total > 0 {
            self.close_requested = true;
            return Ok(());
        }
        if let Some(first) = self.pending.front() {
            // Só o resto não alinhado erra (a spec com `min`): um descritor parcial alinhado fica pendente, e o
            // `read(view, { min })` dele nunca se resolve (medido no bun, índice 503).
            if first.bytes_filled % first.element_size != 0 {
                return Err("insufficient bytes to fill elements in the given buffer");
            }
        }
        self.close_requested = true;
        self.closed = true;
        out.push(Action::CloseStream);
        // Os `pullInto` pendentes ficam: só `respond(0)` os conclui (RespondInClosedState).
        Ok(())
    }

    /// `respond(bytesWritten)` com `bytes_written` já validado como inteiro não negativo.
    pub(super) fn respond(&mut self, bytes_written: usize, out: &mut Vec<Action>) -> Result<(), &'static str> {
        let Some(first) = self.pending.front().copied() else {
            // Fechado e já concluído: `respond(0)` não tem mais o que fazer.
            return if self.closed && bytes_written == 0 { Ok(()) } else { Err(msg::INVALIDATED) };
        };
        self.check_written(bytes_written, first.bytes_filled + bytes_written > first.byte_length)?;
        self.respond_internal(bytes_written, out);
        Ok(())
    }

    /// As validações comuns de `respond` e `respondWithNewView`: fechado exige zero, aberto exige mais que zero e
    /// que caiba (`too_many`).
    fn check_written(&self, bytes_written: usize, too_many: bool) -> Result<(), &'static str> {
        if self.closed {
            if bytes_written != 0 {
                return Err(msg::CLOSED_NONZERO);
            }
        } else if bytes_written == 0 {
            return Err(msg::RESPOND_ZERO);
        } else if too_many {
            return Err(msg::RESPOND_TOO_MANY);
        }
        Ok(())
    }

    /// `respondWithNewView(view)`: `data` é o conteúdo inteiro do buffer da view (transferido), `byte_offset` e
    /// `byte_length` a posição dela nele.
    pub(super) fn respond_with_new_view(&mut self, data: Vec<u8>, byte_offset: usize, byte_length: usize, out: &mut Vec<Action>) -> Result<(), &'static str> {
        let Some(first) = self.pending.front().copied() else { return Err(msg::INVALIDATED) };
        self.check_written(byte_length, false)?;
        if first.byte_offset + first.bytes_filled != byte_offset {
            return Err(msg::NEW_VIEW_POSITION);
        }
        if first.buffer_byte_length != data.len() {
            return Err(msg::NEW_VIEW_LENGTH);
        }
        if first.bytes_filled + byte_length > first.byte_length {
            return Err(msg::RESPOND_TOO_MANY);
        }
        self.buffers.replace(first.buffer, data);
        self.respond_internal(byte_length, out);
        Ok(())
    }

    /// `ReadableByteStreamControllerRespondInternal` depois das validações.
    fn respond_internal(&mut self, bytes_written: usize, out: &mut Vec<Action>) {
        out.push(Action::InvalidateByobRequest);
        if self.closed {
            while let Some(mut first) = self.pending.pop_front() {
                first.byte_length = first.bytes_filled;
                self.commit(first, out);
            }
            return;
        }
        let Some(first) = self.pending.front_mut() else { return };
        first.bytes_filled += bytes_written;
        let descriptor = *first;
        if descriptor.bytes_filled < descriptor.minimum_fill {
            return;
        }
        self.pending.pop_front();
        let remainder = descriptor.bytes_filled % descriptor.element_size;
        let mut committed = descriptor;
        if remainder > 0 {
            let end = descriptor.byte_offset + descriptor.bytes_filled;
            let start = end - remainder;
            let tail = self.buffers.bytes(descriptor.buffer)[start..end].to_vec();
            let buffer = self.buffers.alloc(tail);
            self.enqueue_chunk_to_queue(Chunk { buffer, offset: 0, length: remainder });
            committed.bytes_filled -= remainder;
        }
        self.commit(committed, out);
        self.process_pull_into_descriptors_using_queue(out);
    }

    /// O `byobRequest`: a view sobre o descritor da frente (`buffer`, `byte_offset + bytes_filled`, o que resta).
    pub(super) fn byob_request_view(&self) -> Option<(BufferId, usize, usize, u32)> {
        let first = self.pending.front()?;
        Some((first.buffer, first.byte_offset + first.bytes_filled, first.byte_length - first.bytes_filled, first.view_kind))
    }

    /// `autoAllocateChunkSize`: o descritor que um `read()` padrão cria para o `pull` do usuário.
    pub(super) fn push_auto_allocate(&mut self, size: usize, view_kind: u32) {
        let buffer = self.buffers.alloc(vec![0; size]);
        self.pending.push_back(PullInto {
            buffer,
            buffer_byte_length: size,
            byte_offset: 0,
            byte_length: size,
            bytes_filled: 0,
            minimum_fill: 1,
            element_size: 1,
            view_kind,
            reader: ReaderKind::Default,
        });
    }
}

fn unreachable_descriptor() -> PullInto {
    PullInto {
        buffer: BufferId(0),
        buffer_byte_length: 0,
        byte_offset: 0,
        byte_length: 0,
        bytes_filled: 0,
        minimum_fill: 1,
        element_size: 1,
        view_kind: 0,
        reader: ReaderKind::None,
    }
}
