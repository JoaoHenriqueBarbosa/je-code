//! Janela circular do descompressor: guarda os últimos `window_size` bytes decodificados para as
//! cópias de sequência (`offset` até `window_size`). Cresce sob demanda até o teto.

pub struct Window {
    buf: Vec<u8>,
    cap: usize,
    /// Próxima posição de escrita quando o anel já deu a volta.
    pos: usize,
}

impl Window {
    pub fn new(window_size: usize) -> Self {
        Window { buf: Vec::new(), cap: window_size.max(1), pos: 0 }
    }

    pub fn push_byte(&mut self, b: u8) {
        if self.buf.len() < self.cap {
            self.buf.push(b);
        } else {
            self.buf[self.pos] = b;
            self.pos = (self.pos + 1) % self.cap;
        }
    }

    pub fn push_slice(&mut self, data: &[u8]) {
        for &b in data {
            self.push_byte(b);
        }
    }

    /// Bytes disponíveis para trás.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// O byte `distance` posições atrás do próximo a escrever (1 = o último escrito).
    pub fn back(&self, distance: usize) -> Option<u8> {
        if distance == 0 || distance > self.buf.len() {
            return None;
        }
        let n = self.buf.len();
        let idx = if n < self.cap { n - distance } else { (self.pos + n - distance) % n };
        Some(self.buf[idx])
    }

    /// Cópia de sequência (`ZSTD_overlapCopy8`): repete `len` bytes de `offset` atrás, byte a byte
    /// para a sobreposição funcionar, anexando também em `out`. `None` se o offset passa da janela.
    pub fn copy_match(&mut self, offset: usize, len: usize, out: &mut Vec<u8>) -> Option<()> {
        for _ in 0..len {
            let b = self.back(offset)?;
            self.push_byte(b);
            out.push(b);
        }
        Some(())
    }
}
