//! Porte de `src/vs/platform/files/common/io.ts`: ler um arquivo para dentro de um stream usando
//! `open`/`read`/`close` do provider.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;

use je_base::buffer::VSBuffer;
use je_base::stream::WriteableStream;
use super::files::{
    FileOpenOptions, FileReadStreamOptions, FileSystemProvider, FileSystemProviderErrorCode, create_file_system_provider_error_from_text,
    ensure_file_system_provider_error,
};

/// `IErrorTransformer`.
pub type ErrorTransformer = Rc<dyn Fn(&Error) -> Error>;

/// `ICreateReadStreamOptions`.
#[derive(Clone)]
pub struct CreateReadStreamOptions {
    pub stream: FileReadStreamOptions,
    pub buffer_size: usize,
    pub error_transformer: Option<ErrorTransformer>,
}

/// `readFileIntoStream(provider, resource, target, transformer, options, token)`; o transformador do
/// original é sempre a identidade nos dois usos, então não existe aqui.
pub fn read_file_into_stream(
    provider: Rc<dyn FileSystemProvider>,
    resource: URI,
    target: Rc<dyn WriteableStream<VSBuffer>>,
    options: CreateReadStreamOptions,
    token: CancellationToken,
) -> Promise<()> {
    async_fn(async move {
        let result = do_read_file_into_stream(&provider, &resource, &target, &options, &token).await;
        if let Err(error) = result {
            let error = match &options.error_transformer {
                Some(transformer) => transformer(&error),
                None => error,
            };
            target.error(error);
        }
        target.end(None);
        Ok(())
    })
}

async fn do_read_file_into_stream(
    provider: &Rc<dyn FileSystemProvider>,
    resource: &URI,
    target: &Rc<dyn WriteableStream<VSBuffer>>,
    options: &CreateReadStreamOptions,
    token: &CancellationToken,
) -> Result<(), Error> {
    throw_if_cancelled(token)?;
    let handle = provider.open(resource, &FileOpenOptions::for_read()).await?;
    let result = read_loop(provider, handle, target, options, token).await;
    let closed = provider.close(handle).await;
    // O `finally { await provider.close(handle) }` do original: um erro do `close` vence o do corpo.
    closed?;
    result.map_err(|error| ensure_file_system_provider_error(Some(error)))
}

async fn read_loop(
    provider: &Rc<dyn FileSystemProvider>,
    handle: i64,
    target: &Rc<dyn WriteableStream<VSBuffer>>,
    options: &CreateReadStreamOptions,
    token: &CancellationToken,
) -> Result<(), Error> {
    throw_if_cancelled(token)?;
    let mut total_bytes_read: u64 = 0;
    let mut allowed_remaining = options.stream.length;
    let chunk_size = |remaining: Option<u64>| -> usize {
        match remaining {
            Some(remaining) => options.buffer_size.min(usize::try_from(remaining).unwrap_or(usize::MAX)),
            None => options.buffer_size,
        }
    };
    let mut buffer = VSBuffer::alloc(chunk_size(allowed_remaining));
    let mut pos_in_file = options.stream.position.unwrap_or(0);
    let mut pos_in_buffer = 0usize;
    loop {
        let bytes_read = provider.read(handle, pos_in_file, &buffer, pos_in_buffer, buffer.byte_length() - pos_in_buffer).await?;
        pos_in_file += bytes_read as u64;
        pos_in_buffer += bytes_read;
        total_bytes_read += bytes_read as u64;
        if let Some(remaining) = allowed_remaining.as_mut() {
            *remaining = remaining.saturating_sub(bytes_read as u64);
        }
        if pos_in_buffer == buffer.byte_length()
            && let Some(pending) = target.write(buffer.clone())
        {
            pending.await?;
        }
        if pos_in_buffer == buffer.byte_length() {
            buffer = VSBuffer::alloc(chunk_size(allowed_remaining));
            pos_in_buffer = 0;
        }
        let keep_going = bytes_read > 0 && allowed_remaining.is_none_or(|remaining| remaining > 0);
        if !keep_going {
            break;
        }
        throw_if_cancelled(token)?;
        throw_if_too_large(total_bytes_read, options)?;
    }
    if pos_in_buffer > 0 {
        let mut last_chunk_length = pos_in_buffer;
        if let Some(remaining) = allowed_remaining {
            last_chunk_length = last_chunk_length.min(usize::try_from(remaining).unwrap_or(usize::MAX));
        }
        target.write(buffer.slice(Some(0), Some(last_chunk_length as i64)));
    }
    Ok(())
}

fn throw_if_cancelled(token: &CancellationToken) -> Result<(), Error> {
    if token.is_cancellation_requested() {
        return Err(Error::cancellation());
    }
    Ok(())
}

fn throw_if_too_large(total_bytes_read: u64, options: &CreateReadStreamOptions) -> Result<(), Error> {
    if options.stream.limit_size.is_some_and(|limit| total_bytes_read > limit) {
        return Err(create_file_system_provider_error_from_text(
            &localize("fileTooLargeError", "File is too large to open", &[]),
            FileSystemProviderErrorCode::FileTooLarge,
        ));
    }
    Ok(())
}
