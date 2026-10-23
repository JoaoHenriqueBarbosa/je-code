//! Porte de `src/vs/base/parts/storage/node/storage.ts`: `SQLiteStorageDatabase` sobre `turso_core`
//! (SQLite em Rust puro, hipótese H12) no lugar do `@vscode/sqlite3`.
//!
//! O original fala com o `sqlite3` por callbacks que voltam numa macrotarefa; o `turso_core` é
//! síncrono. Cada operação sobre a conexão roda dentro de uma macrotarefa (`in_macrotask`), então a
//! ordem observável é a do original: o chamador recebe a promessa antes de o trabalho acontecer, e o
//! resultado chega depois das microtarefas pendentes.
//!
//! Diferenças deliberadas frente ao `sqlite3`, todas por causa do `turso_core`:
//! - o banco é sempre WAL; antes de fechar fazemos um checkpoint `TRUNCATE` (o que o `sqlite3` faz no
//!   último `close` em modo WAL) e removemos o `-wal` vazio, para o arquivo principal ficar completo
//!   antes do `.backup`;
//! - `VACUUM` é recurso experimental do `turso_core` e é ligado nas opções do banco;
//! - o código de erro (`SQLITE_BUSY`, `SQLITE_NOTADB`...) sai do código de resultado do `turso_core`.

use std::cell::{Cell, OnceCell, RefCell};
use std::num::NonZero;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use je_base::errors::Error;
use je_base::event::Event;
use je_base::map::{map_to_string, set_to_string};
use je_base::promise::Promise;
use turso_core::{
    CheckpointMode, ConnectOpts, Connection, Database, DatabaseOpts, IO, LimboError, MemoryIO,
    OpenOptions, PlatformIO, SqliteDialect, Value,
};

use crate::async_utils::{in_macrotask, spawn_promise, timeout};
use crate::storage::{ItemMap, Recovery, StorageDatabase, StorageItemsChangeEvent, UpdateRequest};

/// Uma função de log: recebe a mensagem.
pub type LogFn = Rc<dyn Fn(&str)>;

/// `ISQLiteStorageDatabaseLoggingOptions`.
#[derive(Clone, Default)]
pub struct SqliteStorageDatabaseLoggingOptions {
    pub log_error: Option<LogFn>,
    pub log_trace: Option<LogFn>,
}

/// `ISQLiteStorageDatabaseOptions`.
#[derive(Clone, Default)]
pub struct SqliteStorageDatabaseOptions {
    pub logging: Option<SqliteStorageDatabaseLoggingOptions>,
    pub use_wal: bool,
    /// Quando definido, o `busy_timeout` do SQLite em milissegundos.
    pub busy_timeout: Option<u64>,
}

/// `SQLiteStorageDatabaseLogger`.
struct Logger {
    log_trace: Option<LogFn>,
    log_error: Option<LogFn>,
}

impl Logger {
    /// Para reduzir a saída, o trace exige a variável de ambiente (ajuda ao rodar com `--verbose`).
    const VSCODE_TRACE_STORAGE: &'static str = "VSCODE_TRACE_STORAGE";

    fn new(options: Option<SqliteStorageDatabaseLoggingOptions>) -> Self {
        let options = options.unwrap_or_default();
        let tracing_enabled =
            std::env::var(Self::VSCODE_TRACE_STORAGE).is_ok_and(|v| !v.is_empty());
        Self {
            log_trace: options.log_trace.filter(|_| tracing_enabled),
            log_error: options.log_error,
        }
    }

    fn is_tracing(&self) -> bool {
        self.log_trace.is_some()
    }

    fn trace(&self, msg: &str) {
        if let Some(log_trace) = &self.log_trace {
            log_trace(msg);
        }
    }

    fn error(&self, error: &str) {
        if let Some(log_error) = &self.log_error {
            log_error(error);
        }
    }
}

struct Handle {
    // O `Arc<Database>` precisa viver enquanto a conexão vive, e cair junto no `close`: o registro de
    // bancos do turso é por caminho, e um `Database` esquecido devolveria um arquivo já substituído.
    _database: Arc<Database>,
    connection: Arc<Connection>,
}

/// `IDatabaseConnection`.
struct DatabaseConnection {
    handle: RefCell<Option<Handle>>,
    is_in_memory: bool,
    is_erroneous: Cell<bool>,
    last_error: RefCell<Option<String>>,
}

type Conn = Rc<DatabaseConnection>;

// `Promise<T>` exige `T: Clone`, e `Rc` é.
impl DatabaseConnection {
    fn with_handle<T>(
        &self,
        work: impl FnOnce(&Arc<Connection>) -> Result<T, LimboError>,
    ) -> Result<T, Error> {
        match &*self.handle.borrow() {
            Some(handle) => work(&handle.connection).map_err(|e| sqlite_error(&e)),
            None => {
                Err(Error::new("SQLITE_MISUSE: Database is closed")
                    .with_prop("code", "SQLITE_MISUSE"))
            }
        }
    }

    /// `db.close(cb)`: devolve o erro, se houve. O checkpoint `TRUNCATE` leva o conteúdo do WAL para o
    /// arquivo principal (ver o comentário do módulo).
    fn close_sync(&self, path: &str) -> Option<String> {
        let handle = self.handle.borrow_mut().take()?;
        let mut error = None;
        if !self.is_in_memory
            && let Err(e) = handle.connection.checkpoint(CheckpointMode::Truncate {
                upper_bound_inclusive: None,
            })
        {
            error = Some(e.to_string());
        }
        if let Err(e) = handle.connection.close() {
            error.get_or_insert_with(|| e.to_string());
        }
        drop(handle);
        if error.is_none() && !self.is_in_memory {
            let _ = std::fs::remove_file(wal_path(path));
        }
        error
    }
}

fn wal_path(path: &str) -> String {
    format!("{path}-wal")
}

/// O `error.code` do `@vscode/sqlite3`, a partir do código de resultado primário do SQLite.
fn sqlite_code(error: &LimboError) -> &'static str {
    match error.sqlite_result_code() {
        5 => "SQLITE_BUSY",
        6 => "SQLITE_LOCKED",
        8 => "SQLITE_READONLY",
        11 => "SQLITE_CORRUPT",
        13 => "SQLITE_FULL",
        26 => "SQLITE_NOTADB",
        _ => "SQLITE_ERROR",
    }
}

fn sqlite_error(error: &LimboError) -> Error {
    let code = sqlite_code(error);
    Error::new(format!("{code}: {error}")).with_prop("code", code)
}

fn io_error(error: &std::io::Error) -> Error {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        _ => "EIO",
    };
    Error::new(format!("{code}: {error}")).with_prop("code", code)
}

/// A abertura que falha por não achar o diretório ou o arquivo é `SQLITE_CANTOPEN` no `sqlite3`.
fn open_error(error: &LimboError) -> Error {
    match error {
        LimboError::CompletionError(turso_core::CompletionError::IOError(
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied,
            _,
        )) => Error::new(format!(
            "SQLITE_CANTOPEN: unable to open database file ({error})"
        ))
        .with_prop("code", "SQLITE_CANTOPEN"),
        other => sqlite_error(other),
    }
}

/// `chunks` do original: o vetor semente começa com um pedaço vazio e o pedaço novo só abre quando o
/// atual já passou de `MAX_HOST_PARAMETERS` itens (então cada pedaço leva até 257 itens).
fn split_chunks<T>(items: impl IntoIterator<Item = Vec<T>>, max: usize) -> Vec<Vec<T>> {
    let mut chunks: Vec<Vec<T>> = vec![Vec::new()];
    for item in items {
        if chunks.last().is_some_and(|chunk| chunk.len() > max) {
            chunks.push(Vec::new());
        }
        chunks.last_mut().expect("há sempre um pedaço").extend(item);
    }
    chunks
}

struct PreparedStatement {
    sql: String,
    params: Vec<String>,
    is_insert: bool,
}

/// `SQLiteStorageDatabase`.
pub struct SqliteStorageDatabase {
    me: Weak<SqliteStorageDatabase>,
    path: String,
    name: String,
    logger: Logger,
    use_wal: bool,
    busy_timeout: Option<u64>,
    when_connected: OnceCell<Promise<Conn>>,
}

impl SqliteStorageDatabase {
    pub const IN_MEMORY_PATH: &'static str = ":memory:";

    /// Timeout em ms para tentar de novo quando a abertura falha com `SQLITE_BUSY`.
    const BUSY_OPEN_TIMEOUT: u64 = 2000;
    /// Número máximo de parâmetros dentro de um statement.
    const MAX_HOST_PARAMETERS: usize = 256;

    pub fn new(path: &str, options: SqliteStorageDatabaseOptions) -> Rc<Self> {
        let name = Path::new(path)
            .file_name()
            .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
        let database = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            path: path.to_string(),
            name,
            logger: Logger::new(options.logging),
            use_wal: options.use_wal,
            busy_timeout: options.busy_timeout,
            when_connected: OnceCell::new(),
        });
        let connected = database.connect(path.to_string(), true);
        database
            .when_connected
            .set(connected)
            .unwrap_or_else(|_| unreachable!("definido uma vez só, no construtor"));
        database
    }

    fn this(&self) -> Rc<Self> {
        self.me
            .upgrade()
            .expect("o banco está vivo enquanto um método dele roda")
    }

    fn when_connected(&self) -> Promise<Conn> {
        self.when_connected
            .get()
            .expect("definido no construtor")
            .clone()
    }

    fn to_backup_path(path: &str) -> String {
        format!("{path}.backup")
    }

    fn handle_sqlite_error(&self, connection: &Conn, msg: &str) {
        connection.is_erroneous.set(true);
        *connection.last_error.borrow_mut() = Some(msg.to_string());

        self.logger.error(msg);
    }

    /// `fs.promises.unlink(path)`; o `-wal` que o turso deixa ao lado também sai, para não valer num
    /// banco novo com o mesmo nome.
    fn unlink_database(path: String) -> Promise<()> {
        in_macrotask(move || {
            std::fs::remove_file(&path).map_err(|e| io_error(&e))?;
            let _ = std::fs::remove_file(wal_path(&path));
            Ok(())
        })
    }

    pub fn check_integrity(&self, full: bool) -> Promise<String> {
        let this = self.this();
        spawn_promise(async move {
            this.logger.trace(&format!(
                "[storage {}] checkIntegrity(full: {full})",
                this.name
            ));

            let connection = this.when_connected().await?;
            let integrity = this
                .get(
                    &connection,
                    if full {
                        "PRAGMA integrity_check"
                    } else {
                        "PRAGMA quick_check"
                    },
                )
                .await?;

            if connection.is_erroneous.get() {
                return Ok(format!(
                    "{integrity} (last error: {})",
                    connection.last_error.borrow().clone().unwrap_or_default()
                ));
            }

            if connection.is_in_memory {
                return Ok(format!("{integrity} (in-memory!)"));
            }

            Ok(integrity)
        })
    }

    fn do_update_items(&self, connection: &Conn, request: UpdateRequest) -> Promise<()> {
        if self.logger.is_tracing() {
            self.logger.trace(&format!(
                "[storage {}] updateItems(): insert({}), delete({})",
                self.name,
                request
                    .insert
                    .as_ref()
                    .map_or_else(|| "0".to_string(), map_to_string),
                request
                    .delete
                    .as_ref()
                    .map_or_else(|| "0".to_string(), set_to_string)
            ));
        }

        let mut statements = Vec::new();

        // INSERT
        if let Some(to_insert) = request.insert.as_ref().filter(|insert| !insert.is_empty()) {
            // Divide chaves e valores em pedaços de MAX_HOST_PARAMETERS, para rodar o INSERT com o
            // maior número possível de parâmetros.
            let pairs = to_insert
                .iter()
                .map(|(key, value)| vec![key.clone(), value.clone()]);
            for chunk in split_chunks(pairs, Self::MAX_HOST_PARAMETERS) {
                let values = vec!["(?,?)"; chunk.len() / 2].join(",");
                statements.push(PreparedStatement {
                    sql: format!("INSERT INTO ItemTable VALUES {values} ON CONFLICT (key) DO UPDATE SET value = excluded.value WHERE value != excluded.value"),
                    params: chunk,
                    is_insert: true,
                });
            }
        }

        // DELETE
        if let Some(to_delete) = request.delete.as_ref().filter(|delete| !delete.is_empty()) {
            // Divide as chaves em pedaços de MAX_HOST_PARAMETERS, para rodar o DELETE com o maior
            // número possível de parâmetros.
            let keys = to_delete.keys().map(|key| vec![key.clone()]);
            for chunk in split_chunks(keys, Self::MAX_HOST_PARAMETERS) {
                let placeholders = vec!["?"; chunk.len()].join(",");
                statements.push(PreparedStatement {
                    sql: format!("DELETE FROM ItemTable WHERE key IN ({placeholders})"),
                    params: chunk,
                    is_insert: false,
                });
            }
        }

        let this = self.this();
        let connection = connection.clone();
        in_macrotask(move || this.transaction(&connection, &statements, &request))
    }

    /// `transaction`: `BEGIN`, os statements em sequência e `END`. Um statement que falha marca a conexão
    /// como com erro e a transação segue, como no original (o erro do `END` é o que rejeita).
    fn transaction(
        &self,
        connection: &Conn,
        statements: &[PreparedStatement],
        request: &UpdateRequest,
    ) -> Result<(), Error> {
        let begin = connection.with_handle(|c| c.execute("BEGIN TRANSACTION"));
        if let Err(error) = begin {
            self.handle_sqlite_error(
                connection,
                &format!("[storage {}] transaction(): {error}", self.name),
            );
            return Err(error);
        }

        for statement in statements {
            self.run_prepared(connection, statement, request);
        }

        let end = connection.with_handle(|c| c.execute("END TRANSACTION"));
        if let Err(error) = end {
            self.handle_sqlite_error(
                connection,
                &format!("[storage {}] transaction(): {error}", self.name),
            );
            return Err(error);
        }
        Ok(())
    }

    /// `prepare`: um erro de preparo ou de execução só marca a conexão (o `stmt.on('error')` do original).
    fn run_prepared(
        &self,
        connection: &Conn,
        statement: &PreparedStatement,
        request: &UpdateRequest,
    ) {
        let result = connection.with_handle(|c| {
            let mut stmt = c.prepare(&statement.sql)?;
            for (index, param) in statement.params.iter().enumerate() {
                stmt.bind_at(
                    NonZero::new(index + 1).expect("índice a partir de 1"),
                    Value::from_text(param.clone()),
                )?;
            }
            stmt.run_ignore_rows()
        });
        if let Err(error) = result {
            self.handle_sqlite_error(
                connection,
                &format!(
                    "[storage {}] prepare(): {error} ({}). Details: {}",
                    self.name,
                    statement.sql,
                    Self::error_details(statement.is_insert, request)
                ),
            );
        }
    }

    fn error_details(is_insert: bool, request: &UpdateRequest) -> String {
        if is_insert {
            let insert = request.insert.clone().unwrap_or_default();
            let keys: Vec<&str> = insert.keys().map(String::as_str).collect();
            let length: usize = insert
                .values()
                .map(|value| value.encode_utf16().count())
                .sum();
            format!("Keys: {} Length: {length}", keys.join(", "))
        } else {
            let delete = request.delete.clone().unwrap_or_default();
            let keys: Vec<&str> = delete.keys().map(String::as_str).collect();
            format!("Keys: {}", keys.join(", "))
        }
    }

    fn do_close(&self, connection: Conn, recovery: Option<Recovery>) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            let close_error = {
                let connection = connection.clone();
                let path = this.path.clone();
                in_macrotask(move || Ok(connection.close_sync(&path))).await?
            };
            if let Some(close_error) = &close_error {
                this.handle_sqlite_error(
                    &connection,
                    &format!("[storage {}] close(): {close_error}", this.name),
                );
            }

            // Sai cedo se o storage foi criado só em memória, por exemplo nos testes, que não precisam de
            // backup.
            if this.path == Self::IN_MEMORY_PATH {
                return Ok(());
            }

            // Se o banco fechou bem, não estamos em memória e ele não teve erros durante a execução,
            // faz um backup para servir de fallback caso o banco real corrompa no futuro.
            if !connection.is_erroneous.get() && !connection.is_in_memory {
                if let Err(error) = this.backup().await {
                    this.logger
                        .error(&format!("[storage {}] backup(): {error}", this.name));
                }
                return Ok(()); // ignora backup que falha
            }

            // Recuperação: se detectamos erros no uso do banco, ou estamos num banco em memória (o
            // fallback de não conseguir abrir o banco), e há uma função de recuperação, recriamos o
            // banco com esses dados para recuperar tudo que se sabe sem perda, se possível.
            if let Some(recovery) = recovery {
                // Apaga o banco existente. Se o caminho não existe ou não dá para apagar, não tentamos
                // mais recuperar, porque supomos que o caminho não é mais gravável.
                Self::unlink_database(this.path.clone()).await?;

                // Reabre o banco do zero.
                let recovery_connection = this.do_connect(&this.path).await?;

                // Guarda os itens.
                let request = UpdateRequest {
                    insert: Some(recovery()),
                    delete: None,
                };
                return match this.do_update_items(&recovery_connection, request).await {
                    // não tenta recuperar de novo
                    Ok(()) => this.do_close(recovery_connection, None).await,
                    Err(error) => {
                        // Num erro ao atualizar os itens, ainda assim fecha a conexão para evitar
                        // SQLITE_BUSY quando a conexão for reestabelecida.
                        drop(this.do_close(recovery_connection, None));
                        Err(error)
                    }
                };
            }

            // Sem recuperação, só rejeita.
            Err(close_error.map_or_else(
                || Error::new("Database has errors or is in-memory without recovery option"),
                Error::new,
            ))
        })
    }

    fn backup(&self) -> Promise<()> {
        let path = self.path.clone();
        let backup_path = Self::to_backup_path(&path);
        // `Promises.copy(..., { preserveSymlinks: false })`
        in_macrotask(move || {
            std::fs::copy(&path, &backup_path)
                .map(|_| ())
                .map_err(|e| io_error(&e))
        })
    }

    fn connect(&self, path: String, retry_on_busy: bool) -> Promise<Conn> {
        let this = self.this();
        spawn_promise(async move {
            this.logger.trace(&format!(
                "[storage {}] open({path}, retryOnBusy: {retry_on_busy})",
                this.name
            ));

            let error = match this.do_connect(&path).await {
                Ok(connection) => return Ok(connection),
                Err(error) => error,
            };
            this.logger.error(&format!(
                "[storage {}] open(): Unable to open DB due to {error}",
                this.name
            ));

            // SQLITE_BUSY só deve surgir se outro processo trava o mesmo banco que queremos abrir. Isso
            // quase nunca acontece porque a conexão é uma por janela, mas num reload da janela a conexão
            // anterior pode não ter sido fechada direito enquanto a nova já abre.
            //
            // Nesse caso esperamos um pouco e tentamos mais uma vez.
            if error.prop("code") == Some("SQLITE_BUSY") && retry_on_busy {
                timeout(Self::BUSY_OPEN_TIMEOUT).await?;

                return this.connect(path, false /* sem outra tentativa */).await;
            }

            // Senão, o melhor que dá é recuperar de um backup, se existir: movemos o banco para outro
            // nome e tentamos carregar do backup. Se isso falha, um banco vazio novo é criado sozinho.
            //
            // O último fallback é um banco em memória, que só deve acontecer se a pasta de destino
            // realmente não é gravável.
            let attempt = async {
                Self::unlink_database(path.clone()).await?;
                // `Promises.rename(backup, path, false)`: um backup que não existe é ignorado.
                let (from, to) = (Self::to_backup_path(&path), path.clone());
                let _ = in_macrotask(move || std::fs::rename(&from, &to).map_err(|e| io_error(&e)))
                    .await;

                this.do_connect(&path).await
            };
            match attempt.await {
                Ok(connection) => Ok(connection),
                Err(error) => {
                    this.logger.error(&format!(
                        "[storage {}] open(): Unable to use backup due to {error}",
                        this.name
                    ));

                    // Num erro ao abrir o banco, usa um banco em memória para sempre haver um banco
                    // válido com que falar.
                    this.do_connect(Self::IN_MEMORY_PATH).await
                }
            }
        })
    }

    fn do_connect(&self, path: &str) -> Promise<Conn> {
        let this = self.this();
        let path = path.to_string();
        in_macrotask(move || {
            let is_in_memory = path == Self::IN_MEMORY_PATH;
            let io: Arc<dyn IO> = if is_in_memory {
                Arc::new(MemoryIO::new())
            } else {
                Arc::new(PlatformIO::new().map_err(|e| sqlite_error(&e))?)
            };
            let options = OpenOptions::new(Arc::new(SqliteDialect))
                .db_opts(DatabaseOpts::new().with_vacuum(true));
            let database = Database::open(io, &path, options).map_err(|e| open_error(&e))?;
            let connection = database
                .connect(ConnectOpts::new())
                .map_err(|e| open_error(&e))?;
            let connection = Rc::new(DatabaseConnection {
                handle: RefCell::new(Some(Handle {
                    _database: database,
                    connection,
                })),
                is_in_memory,
                is_erroneous: Cell::new(false),
                last_error: RefCell::new(None),
            });

            // O exec abaixo serve a dois propósitos:
            // - criar o banco se ele ainda não existe
            // - validar que o banco não está corrompido (o open não lança senão)
            let mut pragmas = vec!["PRAGMA user_version = 1;".to_string(), "CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);".to_string()];
            if this.use_wal {
                pragmas.push("PRAGMA journal_mode=WAL;".to_string());
            }
            if let Some(busy_timeout) = this.busy_timeout.filter(|ms| *ms != 0) {
                pragmas.push(format!("PRAGMA busy_timeout={busy_timeout};"));
            }
            let sql = pragmas.concat();
            if let Err(error) = this.exec_sync(&connection, &sql) {
                connection.close_sync(&path);
                return Err(error);
            }
            Ok(connection)
        })
    }

    fn exec_sync(&self, connection: &Conn, sql: &str) -> Result<(), Error> {
        connection
            .with_handle(|c| c.execute(sql))
            .inspect_err(|error| {
                self.handle_sqlite_error(
                    connection,
                    &format!("[storage {}] exec(): {error}", self.name),
                )
            })
    }

    fn exec(&self, connection: &Conn, sql: &str) -> Promise<()> {
        let this = self.this();
        let (connection, sql) = (connection.clone(), sql.to_string());
        in_macrotask(move || this.exec_sync(&connection, &sql))
    }

    /// `get`: a primeira coluna da primeira linha (os `PRAGMA` de integridade têm uma só coluna).
    fn get(&self, connection: &Conn, sql: &str) -> Promise<String> {
        let this = self.this();
        let (connection, sql) = (connection.clone(), sql.to_string());
        in_macrotask(move || {
            let rows = connection
                .with_handle(|c| c.prepare(&sql)?.run_collect_rows())
                .inspect_err(|error| {
                    this.handle_sqlite_error(
                        &connection,
                        &format!("[storage {}] get(): {error}", this.name),
                    )
                })?;
            Ok(rows
                .first()
                .and_then(|row| row.first())
                .map(value_to_string)
                .unwrap_or_default())
        })
    }

    /// `all`: as linhas como pares chave e valor.
    fn all(&self, connection: &Conn, sql: &str) -> Promise<Vec<(String, String)>> {
        let this = self.this();
        let (connection, sql) = (connection.clone(), sql.to_string());
        in_macrotask(move || {
            let rows = connection
                .with_handle(|c| c.prepare(&sql)?.run_collect_rows())
                .inspect_err(|error| {
                    this.handle_sqlite_error(
                        &connection,
                        &format!("[storage {}] all(): {error}", this.name),
                    )
                })?;
            Ok(rows
                .iter()
                .map(|row| {
                    (
                        row.first().map(value_to_string).unwrap_or_default(),
                        row.get(1).map(value_to_string).unwrap_or_default(),
                    )
                })
                .collect())
        })
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Text(text) => text.as_str().to_string(),
        Value::Blob(blob) => String::from_utf8_lossy(blob.as_ref()).into_owned(),
        other => other.to_string(),
    }
}

impl StorageDatabase for SqliteStorageDatabase {
    /// Como somos o único cliente, não pode haver mudança externa.
    fn on_did_change_items_external(&self) -> Event<StorageItemsChangeEvent> {
        Event::none()
    }

    fn get_items(&self) -> Promise<ItemMap> {
        let this = self.this();
        spawn_promise(async move {
            let connection = this.when_connected().await?;

            let items = ItemMap::new();

            let rows = this.all(&connection, "SELECT * FROM ItemTable").await?;
            for (key, value) in &rows {
                items.set(key, value);
            }

            if this.logger.is_tracing() {
                this.logger.trace(&format!(
                    "[storage {}] getItems(): {} rows",
                    this.name,
                    items.len()
                ));
            }

            Ok(items)
        })
    }

    fn update_items(&self, request: UpdateRequest) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            let connection = this.when_connected().await?;

            this.do_update_items(&connection, request).await
        })
    }

    fn optimize(&self) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            this.logger
                .trace(&format!("[storage {}] vacuum()", this.name));

            let connection = this.when_connected().await?;

            this.exec(&connection, "VACUUM").await
        })
    }

    fn close(&self, recovery: Option<Recovery>) -> Promise<()> {
        let this = self.this();
        spawn_promise(async move {
            this.logger
                .trace(&format!("[storage {}] close()", this.name));

            let connection = this.when_connected().await?;

            this.do_close(connection, recovery).await
        })
    }
}
