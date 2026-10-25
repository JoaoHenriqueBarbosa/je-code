//! O lado servidor do TLS do proxy: o `https.createServer` do original sobre um `TcpStream`, com o `rustls`
//! (provedor RustCrypto, Rust puro).
//!
//! O `ServerConnection` do `rustls` não é divisível em leitura e escrita; para o túnel (duas threads, uma em cada
//! sentido) a leitura do TCP fica fora da trava: só o processamento dos registros TLS e a escrita pegam a trava da
//! conexão, então uma thread parada esperando bytes nunca segura a outra.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::{ServerConfig, ServerConnection};

use super::tunnel_socket::{Duplex, PendingBytes, lock};

/// Quantos bytes cifrados vêm do TCP de cada vez (um registro TLS cabe).
const CIPHERTEXT_CHUNK: usize = 16 * 1024;

/// Um soquete TLS do lado servidor, já como [`Duplex`] de texto claro.
pub struct TlsServerStream {
    tcp: TcpStream,
    connection: Mutex<ServerConnection>,
    pending: PendingBytes,
    destroyed: AtomicBool,
}

impl TlsServerStream {
    pub fn new(tcp: TcpStream, config: Arc<ServerConfig>) -> io::Result<Arc<Self>> {
        let connection = ServerConnection::new(config).map_err(io::Error::other)?;
        Ok(Arc::new(Self { tcp, connection: Mutex::new(connection), pending: PendingBytes::default(), destroyed: AtomicBool::new(false) }))
    }

    /// `socket.setTimeout`: o prazo de leitura do TCP por baixo (`None` desliga).
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.tcp.set_read_timeout(timeout)
    }

    /// Escreve no TCP tudo o que a conexão TLS tem para enviar (respostas do handshake, alertas, dados).
    fn flush_tls(&self, connection: &mut ServerConnection) -> io::Result<()> {
        let mut tcp = &self.tcp;
        while connection.wants_write() {
            connection.write_tls(&mut tcp)?;
        }
        Ok(())
    }

    /// Entrega bytes cifrados à conexão e processa os registros.
    fn feed(connection: &mut ServerConnection, mut input: &[u8]) -> io::Result<()> {
        while !input.is_empty() {
            let consumed = connection.read_tls(&mut input)?;
            if consumed == 0 {
                break;
            }
            connection.process_new_packets().map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
        Ok(())
    }
}

impl Duplex for TlsServerStream {
    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(count) = self.pending.take(buf) {
            return Ok(count);
        }
        let mut ciphertext = vec![0u8; CIPHERTEXT_CHUNK];
        loop {
            if self.destroyed.load(Ordering::SeqCst) {
                return Ok(0);
            }
            {
                let mut connection = lock(&self.connection);
                match connection.reader().read(buf) {
                    // `Ok(0)` é o `close_notify` do cliente.
                    Ok(count) => return Ok(count),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(0),
                    Err(error) => return Err(error),
                }
            }
            // Sem texto claro pronto: espera bytes cifrados, fora da trava.
            let count = match (&self.tcp).read(&mut ciphertext) {
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if count == 0 {
                return Ok(0);
            }
            let mut connection = lock(&self.connection);
            let processed = Self::feed(&mut connection, &ciphertext[..count]);
            // Um alerta de erro também precisa sair, por isso o flush vem antes de propagar o erro.
            let flushed = self.flush_tls(&mut connection);
            processed?;
            flushed?;
        }
    }

    fn write_all(&self, data: &[u8]) -> io::Result<()> {
        if self.destroyed.load(Ordering::SeqCst) {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        let mut connection = lock(&self.connection);
        let mut rest = data;
        while !rest.is_empty() {
            let written = connection.writer().write(rest)?;
            rest = &rest[written..];
            if written == 0 && !connection.wants_write() {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            }
            self.flush_tls(&mut connection)?;
        }
        Ok(())
    }

    fn end(&self) {
        let mut connection = lock(&self.connection);
        connection.send_close_notify();
        let _ = self.flush_tls(&mut connection);
        let _ = self.tcp.shutdown(Shutdown::Write);
    }

    fn destroy(&self) {
        self.destroyed.store(true, Ordering::SeqCst);
        let _ = self.tcp.shutdown(Shutdown::Both);
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed.load(Ordering::SeqCst)
    }

    fn unshift(&self, data: Vec<u8>) {
        self.pending.unshift(data);
    }
}
