// `Bun.spawn` e `Bun.spawnSync` do bun 1.4.2 sobre o `proc_*` do `Host` (ver `proc_spawn.rs`). Programa escondido: a fábrica
// recebe `host` com os natives `proc*` (processo, leitura e escrita de pipe, espera) e os `net*` (observadores de descritor
// do laço de eventos). O que ainda não foi medido no oráculo está dito no comentário do ponto.
(function (host) {
  "use strict";
  const { procSpawn, procRead, procWrite, procKill, procWait, procPoll, procWatch, procUnwatch, procRef, netWatch, netUnwatch, netClose, netRef } = host;
  const EAGAIN = -11;
  const EPIPE = -32;
  const signals = host.requireBuiltin("node:os").constants.signals;
  const signalName = number => Object.keys(signals).find(name => signals[name] === number) ?? null;

  const invalidArgType = message => {
    const error = new TypeError(message);
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };

  function errnoError(code, errno, message, path) {
    const error = new Error(message);
    error.code = code;
    error.errno = errno;
    if (path !== undefined) error.path = path;
    return error;
  }

  // ---- stdio ----

  const kIgnore = -1;
  const kInherit = -2;
  const kPipe = -3;
  const kSocketPair = -4;

  // Normaliza uma das opções `stdin`/`stdout`/`stderr`/`stdio[i]` para `{ code, input }`: `code` é o que o `procSpawn` entende e
  // `input` os bytes (ou o fluxo) que vão ao stdin do filho depois de criado.
  function stdioOf(value, index, standard) {
    if (value === undefined) value = standard;
    if (value === null || value === "ignore") return { code: kIgnore };
    if (value === "inherit") return { code: kInherit };
    if (value === "pipe") return { code: kPipe };
    if (value === "socket-fd") return { code: kSocketPair };
    if (typeof value === "number") return { code: value };
    if (index === 0) {
      if (typeof value === "string") return { code: kPipe, input: new TextEncoder().encode(value) };
      if (ArrayBuffer.isView(value)) return { code: kPipe, input: new Uint8Array(value.buffer, value.byteOffset, value.byteLength) };
      if (value instanceof ArrayBuffer) return { code: kPipe, input: new Uint8Array(value) };
      if (typeof Blob === "function" && value instanceof Blob) return { code: kPipe, input: value };
      if (typeof ReadableStream === "function" && value instanceof ReadableStream) return { code: kPipe, input: value };
      if (typeof Response === "function" && value instanceof Response) return { code: kPipe, input: value };
    }
    host.internal.throwNotImplemented(`Bun.spawn stdio ${index} of this type`);
  }

  function parseOptions(first, second) {
    let options = {};
    let cmd;
    if (Array.isArray(first)) {
      cmd = first;
      if (second !== undefined && second !== null && typeof second === "object") options = second;
    } else if (first !== null && typeof first === "object") {
      options = first;
      cmd = first.cmd;
      if (cmd === undefined || cmd === null) throw invalidArgType("cmd must be an array");
    } else {
      throw invalidArgType("cmd must be an array");
    }
    if (!Array.isArray(cmd)) throw invalidArgType("cmd must be an array");
    if (cmd.length === 0) throw invalidArgType("cmd must not be empty");
    return { cmd: cmd.map(String), options };
  }

  function plan(cmd, options, sync) {
    const stdio = [];
    const given = Array.isArray(options.stdio) ? options.stdio : [];
    const standard = sync ? ["ignore", "pipe", "pipe"] : ["ignore", "pipe", "inherit"];
    const count = Math.max(3, given.length);
    for (let index = 0; index < count; index++) {
      const own = index < 3 ? [options.stdin, options.stdout, options.stderr][index] : undefined;
      const value = given[index] !== undefined ? given[index] : own;
      stdio.push(stdioOf(value, index, index < 3 ? standard[index] : "ignore"));
    }
    const env = options.env === undefined || options.env === null ? process.env : options.env;
    const pairs = Object.keys(env)
      .filter(key => env[key] !== undefined)
      .map(key => `${key}=${env[key]}`);
    return { file: cmd[0], argv: cmd, env: pairs, cwd: options.cwd === undefined ? "" : String(options.cwd), detached: !!options.detached, stdio };
  }

  function spawnNative(request) {
    const fields = [request.file, request.cwd, request.detached ? "1" : "0", String(request.argv.length), ...request.argv, "1", String(request.env.length), ...request.env, String(request.stdio.length), ...request.stdio.map(entry => String(entry.code))];
    const result = procSpawn(fields.join("\0"));
    if (typeof result === "number") {
      const code = result === -2 ? "ENOENT" : result === -13 ? "EACCES" : result === -8 ? "ENOEXEC" : `E${-result}`;
      if (result === -2) {
        throw errnoError("ENOENT", -2, `Executable not found in $PATH: "${request.file}"`, request.file);
      }
      throw errnoError(code, result, `${code}: ${code === "EACCES" ? "permission denied" : "spawn failed"}, posix_spawn '${request.file}'`, request.file);
    }
    return { pid: result[0], fds: result.slice(1) };
  }

  // ---- leitura do stdout e do stderr ----

  function readableOf(fd, onClosed) {
    let closed = false;
    const close = () => {
      if (closed) return;
      closed = true;
      netClose(fd);
      onClosed();
    };
    return new ReadableStream({
      pull(controller) {
        return new Promise(resolve => {
          const attempt = () => {
            if (closed) {
              resolve();
              return;
            }
            const chunk = procRead(fd, 65536);
            if (typeof chunk === "number") {
              if (chunk === EAGAIN) {
                netWatch(fd, true, false, () => {
                  netUnwatch(fd);
                  attempt();
                });
                return;
              }
              close();
              controller.close();
            } else if (chunk.length === 0) {
              close();
              controller.close();
            } else {
              controller.enqueue(chunk);
            }
            resolve();
          };
          attempt();
        });
      },
      cancel() {
        netUnwatch(fd);
        close();
      },
    });
  }

  // ---- escrita no stdin (`FileSink`) ----

  const sinkStates = new WeakMap();
  const sinkState = sink => {
    const state = sinkStates.get(sink);
    if (state === undefined) throw new TypeError("Expected this to be instanceof FileSink");
    return state;
  };
  function FileSink() {
    throw new TypeError("FileSink is not constructable");
  }
  const bytesOf = chunk =>
    typeof chunk === "string" ? new TextEncoder().encode(chunk) : ArrayBuffer.isView(chunk) ? new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength) : new Uint8Array(chunk);
  // Escreve o que couber; o resto espera o pipe esvaziar (`netWatch` de escrita). Fecha depois de esvaziar quando `ending`.
  function drain(state) {
    while (state.queue.length > 0) {
      const head = state.queue[0];
      const written = procWrite(state.fd, head);
      if (written === EAGAIN) {
        netWatch(state.fd, false, true, () => {
          netUnwatch(state.fd);
          drain(state);
        });
        return;
      }
      if (written < 0) {
        state.queue.length = 0;
        break;
      }
      if (written < head.length) state.queue[0] = head.subarray(written);
      else state.queue.shift();
    }
    if (state.ending && state.queue.length === 0 && state.fd !== undefined) {
      netClose(state.fd);
      state.fd = undefined;
      state.onClosed?.();
    }
  }
  const sinkMethods = {
    close() {
      return this.end();
    },
    flush() {
      const state = sinkState(this);
      const pending = state.queue.reduce((total, chunk) => total + chunk.length, 0);
      if (state.fd !== undefined) drain(state);
      return pending;
    },
    end() {
      const state = sinkState(this);
      const pending = state.queue.reduce((total, chunk) => total + chunk.length, 0);
      state.ending = true;
      if (state.fd !== undefined) drain(state);
      return pending;
    },
    start() {},
    write(chunk) {
      const state = sinkState(this);
      const bytes = bytesOf(chunk);
      if (state.fd === undefined || state.ending) return 0;
      state.queue.push(bytes);
      drain(state);
      return bytes.length;
    },
    ref() {
      const state = sinkState(this);
      if (state.fd !== undefined) netRef(state.fd, true);
    },
    unref() {
      const state = sinkState(this);
      if (state.fd !== undefined) netRef(state.fd, false);
    },
    _getFd() {
      const state = sinkState(this);
      return state.fd === undefined ? -1 : state.fd;
    },
    sinkId() {
      return sinkState(this).id;
    },
  };
  const sinkPrototype = {};
  for (const name of Object.keys(sinkMethods)) {
    Object.defineProperty(sinkPrototype, name, { value: sinkMethods[name], writable: true, configurable: true, enumerable: true });
  }
  Object.defineProperty(sinkPrototype, "constructor", { value: FileSink, writable: true, configurable: true, enumerable: false });
  Object.defineProperty(sinkPrototype, Symbol.toStringTag, { value: "FileSink", configurable: true });
  FileSink.prototype = sinkPrototype;
  let nextSinkId = 1;
  function sinkOf(fd, onClosed) {
    const sink = Object.create(sinkPrototype);
    sinkStates.set(sink, { fd, queue: [], ending: false, onClosed, id: nextSinkId++ });
    return sink;
  }

  // ---- o objeto Subprocess ----

  const states = new WeakMap();
  const stateOf = process => {
    const state = states.get(process);
    if (state === undefined) throw new TypeError("Expected this to be instanceof Subprocess");
    return state;
  };
  const accessor = get => ({ get, configurable: true, enumerable: true });
  const method = value => ({ value, writable: true, configurable: true, enumerable: true });
  const subprocessPrototype = {};
  const members = {
    connected: accessor(function () {
      stateOf(this);
      return false;
    }),
    disconnect: method(function () {
      stateOf(this);
    }),
    exitCode: accessor(function () {
      return stateOf(this).exitCode;
    }),
    exited: accessor(function () {
      return stateOf(this).exited;
    }),
    kill: method(function (signal) {
      const state = stateOf(this);
      let number = 15;
      if (typeof signal === "number") number = signal;
      else if (typeof signal === "string") {
        number = signals[signal];
        if (number === undefined) throw invalidArgType(`Unknown signal: ${signal}`);
      }
      if (state.finished) return;
      state.killed = true;
      procKill(state.pid, number);
    }),
    killed: accessor(function () {
      const state = stateOf(this);
      return state.killed || state.finished;
    }),
    pid: accessor(function () {
      return stateOf(this).pid;
    }),
    readable: accessor(function () {
      return stateOf(this).stdout;
    }),
    ref: method(function () {
      const state = stateOf(this);
      if (!state.finished) procRef(state.pid, true);
    }),
    resourceUsage: method(function () {
      stateOf(this);
      return undefined;
    }),
    send: method(function () {
      stateOf(this);
      host.internal.throwNotImplemented("Subprocess.send");
    }),
    signalCode: accessor(function () {
      return stateOf(this).signalCode;
    }),
    stderr: accessor(function () {
      return stateOf(this).stderr;
    }),
    stdin: accessor(function () {
      return stateOf(this).stdin;
    }),
    stdio: accessor(function () {
      const state = stateOf(this);
      return [state.stdin, state.stdout, state.stderr, ...state.extra];
    }),
    stdout: accessor(function () {
      return stateOf(this).stdout;
    }),
    terminal: accessor(function () {
      stateOf(this);
      return undefined;
    }),
    unref: method(function () {
      const state = stateOf(this);
      if (!state.finished) procRef(state.pid, false);
    }),
    writable: accessor(function () {
      return stateOf(this).stdin;
    }),
  };
  for (const name of Object.keys(members)) Object.defineProperty(subprocessPrototype, name, members[name]);
  Object.defineProperty(subprocessPrototype, Symbol.toStringTag, { value: "Subprocess", configurable: true });

  function pumpInput(sink, input) {
    if (input instanceof Uint8Array) {
      sink.write(input);
      sink.end();
      return;
    }
    const stream = typeof Response === "function" && input instanceof Response ? input.body : typeof Blob === "function" && input instanceof Blob ? input.stream() : input;
    (async () => {
      const reader = stream.getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        sink.write(value);
      }
      sink.end();
    })();
  }

  function spawn(first, second) {
    const { cmd, options } = parseOptions(first, second);
    const request = plan(cmd, options, false);
    const { pid, fds } = spawnNative(request);
    const process = Object.create(subprocessPrototype);
    let settle;
    const exited = new Promise(resolve => {
      settle = resolve;
    });
    const state = { pid, exitCode: null, signalCode: null, killed: false, finished: false, exited, stdin: undefined, stdout: undefined, stderr: undefined, extra: [] };
    states.set(process, state);
    request.stdio.forEach((entry, index) => {
      const fd = fds[index];
      if (index >= 3) {
        if (entry.code === kPipe || entry.code === kSocketPair) state.extra[index - 3] = fd;
        else state.extra[index - 3] = null;
        return;
      }
      if (entry.code !== kPipe || fd < 0) return;
      if (index === 0) {
        state.stdin = sinkOf(fd);
        if (entry.input !== undefined) pumpInput(state.stdin, entry.input);
      } else {
        state[index === 1 ? "stdout" : "stderr"] = readableOf(fd, () => {});
      }
    });
    procWatch(pid, result => {
      state.finished = true;
      const [code, signal] = result;
      state.exitCode = code >= 0 ? code : null;
      state.signalCode = signal >= 0 ? signalName(signal) : null;
      settle(code >= 0 ? code : 128 + signal);
      // Medido no oráculo: quem espera `exited` continua antes de o `onExit` rodar.
      if (typeof options.onExit === "function") queueMicrotask(() => options.onExit(process, state.exitCode, state.signalCode, undefined));
    });
    return process;
  }

  // ---- spawnSync ----

  function spawnSync(first, second) {
    const { cmd, options } = parseOptions(first, second);
    const request = plan(cmd, options, true);
    const { pid, fds } = spawnNative(request);
    const outputs = { 1: [], 2: [] };
    const open = new Set();
    request.stdio.forEach((entry, index) => {
      if (entry.code === kPipe && fds[index] >= 0) open.add(index);
    });
    let input = request.stdio[0].input instanceof Uint8Array ? request.stdio[0].input : undefined;
    if (open.has(0) && input === undefined) {
      netClose(fds[0]);
      open.delete(0);
    }
    const closeFd = index => {
      netClose(fds[index]);
      open.delete(index);
    };
    while (open.size > 0) {
      const indexes = [...open];
      const ready = procPoll(indexes.map(index => `${fds[index]}:${index === 0 ? "w" : "r"}`).join(","), -1);
      if (typeof ready === "number") break;
      indexes.forEach((index, position) => {
        const bits = ready[position];
        if (bits === 0) return;
        if (index === 0) {
          const written = procWrite(fds[0], input);
          if (written === EAGAIN) return;
          if (written < 0 || written >= input.length) {
            closeFd(0);
            return;
          }
          input = input.subarray(written);
          return;
        }
        const chunk = procRead(fds[index], 65536);
        if (typeof chunk === "number") {
          if (chunk !== EAGAIN) closeFd(index);
        } else if (chunk.length === 0) closeFd(index);
        else outputs[index].push(chunk);
      });
    }
    let exit = procWait(pid, false);
    if (typeof exit === "number") exit = [0, -1];
    const [code, signal] = exit;
    const result = { exitCode: code >= 0 ? code : 128 + signal };
    const bufferOf = index => (request.stdio[index].code === kPipe ? Buffer.concat(outputs[index]) : undefined);
    result.stdout = bufferOf(1);
    result.stderr = bufferOf(2);
    result.success = code === 0;
    if (signal >= 0) result.signalCode = signalName(signal);
    result.resourceUsage = {};
    result.pid = pid;
    return result;
  }

  return { spawn, spawnSync };
})
