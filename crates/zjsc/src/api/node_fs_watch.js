// GERADO por `scripts/gen-node-fs-watch.js` a partir de `src/js/internal/fs/watch.ts` e `watchfile.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { codes, validators } = host.internal;
  const kInternalAssertionSuffix =
    "\nThis is caused by either a bug in Node.js or incorrect usage of Node.js internals.\n" +
    "Please open an issue with this stack trace at https://github.com/nodejs/node/issues\n";
  // O `internal/fs/binding` do bun (`watch` e `watchFile` nativos) sobre o `node:fs`: o sysabi não tem inotify, então o
  // observador compara `stat` e `readdir` a cada passo. O que se vê: `change` quando o conteúdo ou os atributos mudam,
  // `rename` quando um nome aparece ou some; a quantidade de eventos de um mesmo gesto é a de uma comparação por
  // passo, não a de uma fila do kernel (declarado em `NOTICE` e no `PLAN`).
  const fsModule = host.requireBuiltin("node:fs");
  const pathModule = host.requireBuiltin("node:path");
  const WATCH_STEP_MS = 50;
  // O `ignore` por texto casa como o `Bun.Glob` do bun: aqui pelo minimatch do `fs.glob`.
  const globMatcher = pattern => host.asset("glob").createMatcher(pattern);
  const uvErrors = new Map([
    [-1, ["EPERM", "operation not permitted"]],
    [-2, ["ENOENT", "no such file or directory"]],
    [-13, ["EACCES", "permission denied"]],
    [-20, ["ENOTDIR", "not a directory"]],
  ]);
  const processBindingUv = { getErrorMap: () => uvErrors };

  const signatureOf = stat => [stat.ino, stat.size, stat.mtimeMs, stat.ctimeMs, stat.mode, stat.nlink].join(":");

  // `{ nome: assinatura }` do que há debaixo de `path` (o próprio arquivo sob o nome vazio), ou null se não existe.
  function snapshot(path, recursive) {
    let stat;
    try {
      stat = fsModule.lstatSync(path);
    } catch {
      return null;
    }
    const entries = { "": signatureOf(stat) };
    if (!stat.isDirectory()) return entries;
    const walk = (directory, prefix) => {
      let names;
      try {
        names = fsModule.readdirSync(directory, { withFileTypes: true });
      } catch {
        return;
      }
      for (const entry of names) {
        const name = prefix + entry.name;
        try {
          entries[name] = signatureOf(fsModule.lstatSync(directory + "/" + entry.name));
        } catch {
          continue;
        }
        if (recursive && entry.isDirectory()) walk(directory + "/" + entry.name, name + "/");
      }
    };
    walk(path, "");
    return entries;
  }

  function watchError(code, path) {
    const entry = [...uvErrors.entries()].find(([, value]) => value[0] === code);
    const error = new Error(`${code}: ${entry[1][1]}, watch '${path}'`);
    error.code = code;
    error.path = path;
    error.syscall = "watch";
    error.errno = entry[0];
    return error;
  }

  const binding = {
    watch(path, options, onEvent) {
      const { persistent = true, recursive = false, encoding = "utf8", signal } = options;
      const resolved = pathModule.resolve(path);
      let before = snapshot(resolved, recursive);
      if (before === null) throw watchError("ENOENT", path);
      const label = name => (encoding === "buffer" ? Buffer.from(name) : name);
      const baseName = pathModule.basename(resolved);
      const directory = fsModule.lstatSync(resolved).isDirectory();
      let closed = false;
      let timer;
      const step = () => {
        const after = snapshot(resolved, recursive);
        if (after === null) {
          if (before !== null) onEvent("rename", label(baseName));
          before = null;
          return;
        }
        if (before === null) {
          onEvent("rename", label(baseName));
          before = after;
          return;
        }
        const names = [...new Set([...Object.keys(before), ...Object.keys(after)])].sort();
        for (const name of names) {
          if (closed) return;
          // O diretório em si não relata a mudança dos filhos.
          if (name === "" && directory) continue;
          const shown = label(name === "" ? baseName : name);
          if (!(name in after) || !(name in before)) onEvent("rename", shown);
          else if (before[name] !== after[name]) onEvent("change", shown);
        }
        before = after;
      };
      const close = () => {
        if (closed) return;
        closed = true;
        clearInterval(timer);
        signal?.removeEventListener("abort", abort);
        onEvent("close");
      };
      const abort = () => {
        if (closed) return;
        onEvent("error", signal.reason, true);
        close();
      };
      timer = setInterval(step, WATCH_STEP_MS);
      if (!persistent) timer.unref?.();
      if (signal) {
        if (signal.aborted) queueMicrotask(abort);
        else signal.addEventListener("abort", abort, { once: true });
      }
      return {
        close,
        ref: () => timer.ref?.(),
        unref: () => timer.unref?.(),
      };
    },

    watchFile(path, options, onChange) {
      const { persistent = true, interval = 5007, bigint = false } = options || {};
      const StatsClass = fsModule.Stats;
      const zeroed = bigint
        ? () => new (fsModule.statSync("/", { bigint: true }).constructor)(...new Array(18).fill(0n))
        : () => new StatsClass(...new Array(14).fill(0));
      const read = () => {
        try {
          return fsModule.statSync(path, { bigint });
        } catch {
          return zeroed();
        }
      };
      const keyOf = stat => [stat.dev, stat.ino, stat.mode, stat.nlink, stat.uid, stat.gid, stat.size, stat.mtimeMs, stat.ctimeMs].join(":");
      let previous = read();
      let first = previous.ino === 0 || previous.ino === 0n;
      const timer = setInterval(() => {
        const current = read();
        // Um arquivo que não existe dá uma chamada com tudo zerado no primeiro passo (como o libuv e o Node).
        if (first || keyOf(current) !== keyOf(previous)) {
          first = false;
          const before = previous;
          previous = current;
          onChange(current, before);
        }
      }, interval);
      if (!persistent) timer.unref?.();
      return {
        close: () => clearInterval(timer),
        ref: () => timer.ref?.(),
        unref: () => timer.unref?.(),
      };
    },
  };
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/fs/binding") return binding;
    if (name === "internal/shared") return { kInternalAssertionSuffix };
    return host.requireBuiltin(name);
  };
  const watchModule = (function () {
    const EventEmitter = require("node:events");
    const { basename } = require("node:path");
    const fs = require("internal/fs/binding");
    function makeGlobMatcher(glob) {
      return function matchGlob(filename) {
        return glob.match(filename);
      };
    }
    function makeGlobOrBasenameMatcher(glob) {
      return function matchGlobOrBasename(filename) {
        return glob.match(filename) || glob.match(basename(filename));
      };
    }
    function makeRegexMatcher(matcher) {
      return function matchRegex(filename) {
        return matcher.exec(filename) !== null;
      };
    }
    function createIgnoreMatcher(ignore) {
      if (ignore == null)
        return null;
      const matchers = Array.isArray(ignore) ? ignore : [ignore];
      const compiled = [];
      for (const matcher of matchers) {
        if (typeof matcher === "string") {
          if (matcher.length === 0) {
            throw codes.INVALID_ARG_VALUE("options.ignore", matcher, "must not be empty");
          }
          const glob = globMatcher(matcher);
          if (matcher.includes("/")) {
            compiled.push(makeGlobMatcher(glob));
          } else {
            compiled.push(makeGlobOrBasenameMatcher(glob));
          }
        } else if (matcher instanceof RegExp) {
          compiled.push(makeRegexMatcher(matcher));
        } else if (typeof matcher === "function") {
          compiled.push(matcher);
        } else {
          throw codes.INVALID_ARG_TYPE("options.ignore", ["string", "RegExp", "Function"], matcher);
        }
      }
      return function isIgnored(filename) {
        if (typeof filename !== "string")
          filename = String(filename);
        for (const match of compiled) {
          if (match(filename))
            return true;
        }
        return false;
      };
    }
    const kFSWatchStart = Symbol("kFSWatchStart");
    let closeNativeWatcher;
    let closeNativeWatcherWithoutEvent;
    let dispatchNativeEvent;
    let refNativeWatcher;
    let unrefNativeWatcher;
    let uvErrorMap;
    function makeWatchUVException(status, filename) {
      uvErrorMap ??= processBindingUv.getErrorMap();
      const entry = uvErrorMap.get(status);
      const code = entry ? entry[0] : "UNKNOWN";
      const description = entry ? entry[1] : "unknown error";
      const err = new Error(`${code}: ${description}, watch '${filename}'`);
      err.errno = status;
      err.code = code;
      err.syscall = "watch";
      err.path = filename;
      err.filename = filename;
      return err;
    }

    class FSEvent {
      #owner;
      constructor(owner) {
        this.#owner = owner;
      }
      onchange(status, eventType, filename) {
        const owner = this.#owner;
        if (status < 0) {
          closeNativeWatcherWithoutEvent(owner);
          owner._handle = null;
          owner.emit("error", makeWatchUVException(status, filename));
          return;
        }
        dispatchNativeEvent(owner, eventType, filename);
      }
      close() {
        closeNativeWatcher(this.#owner);
      }
      ref() {
        refNativeWatcher(this.#owner);
      }
      unref() {
        unrefNativeWatcher(this.#owner);
      }
    }
    function assertFSEventHandle(handle) {
      if (!(handle instanceof FSEvent)) {
        throw codes.INTERNAL_ASSERTION("handle must be a FSEvent" + require("internal/shared").kInternalAssertionSuffix);
      }
    }

    class FSWatcher extends EventEmitter {
      #watcher;
      #listener;
      #ignoreMatcher;
      #silentClose = false;
      _handle;
      constructor(path, options, listener) {
        super();
        if (path instanceof URL) {
          path = host.requireBuiltin("node:url").fileURLToPath(path);
        } else if (typeof path === "string" && path.startsWith("file:")) {
          path = host.requireBuiltin("node:url").fileURLToPath(path);
        }
        if (typeof options === "function") {
          listener = options;
          options = {};
        } else if (typeof options === "string") {
          options = { encoding: options };
        }
        if (typeof listener !== "function") {
          listener = () => {};
        }
        this.#ignoreMatcher = createIgnoreMatcher(options?.ignore);
        this.#listener = listener;
        try {
          this.#watcher = fs.watch(path, options || {}, this.#onEvent.bind(this));
        } catch (e) {
          e.path = path;
          e.filename = path;
          if (options?.throwIfNoEntry !== undefined && !options.throwIfNoEntry && e.code === "ENOENT") {
            this._handle = new FSEvent(this);
            return;
          }
          throw e;
        }
        this._handle = new FSEvent(this);
      }
      #onEvent(eventType, filenameOrError, fromAbort = false) {
        if (eventType === "close") {
          if (this.#silentClose)
            return;
          queueMicrotask(() => {
            this.emit("close", filenameOrError);
          });
          return;
        } else if (eventType === "error") {
          if (fromAbort) {
            if (this.listenerCount("error") === 0)
              return;
          } else if (filenameOrError.code === "EACCES") {
            filenameOrError.code = "EPERM";
          }
          this.emit(eventType, filenameOrError);
        } else {
          if (filenameOrError != null && this.#ignoreMatcher?.(filenameOrError)) {
            return;
          }
          this.emit("change", eventType, filenameOrError);
          this.#listener(eventType, filenameOrError);
        }
      }
      close() {
        if (this._handle === null)
          return;
        assertFSEventHandle(this._handle);
        this._handle.close();
      }
      ref() {
        const handle = this._handle;
        if (handle)
          handle.ref();
        return this;
      }
      unref() {
        const handle = this._handle;
        if (handle)
          handle.unref();
        return this;
      }
      start() {}
      [kFSWatchStart]() {
        if (this._handle === null)
          return;
        assertFSEventHandle(this._handle);
      }
      static {
        dispatchNativeEvent = function dispatchNativeEvent(watcher, eventType, filename) {
          if (filename != null && watcher.#ignoreMatcher?.(filename))
            return;
          watcher.emit("change", eventType, filename);
          watcher.#listener(eventType, filename);
        };
        closeNativeWatcher = function closeNativeWatcher(watcher) {
          watcher.#watcher?.close();
          watcher.#watcher = null;
        };
        closeNativeWatcherWithoutEvent = function closeNativeWatcherWithoutEvent(watcher) {
          watcher.#silentClose = true;
          watcher.#watcher?.close();
          watcher.#watcher = null;
        };
        refNativeWatcher = function refNativeWatcher(watcher) {
          watcher.#watcher?.ref();
        };
        unrefNativeWatcher = function unrefNativeWatcher(watcher) {
          watcher.#watcher?.unref();
        };
      }
    }
    function watch(path, options, listener) {
      return new FSWatcher(path, options, listener);
    }
    return { watch, createIgnoreMatcher };

  })();
  const watchFileModule = (function () {
    const EventEmitter = require("node:events");
    const { getValidatedPath, throwIfNullBytesInFileName } = require("internal/validators");
    const fs = require("internal/fs/binding");
    function emitStop(self) {
      self.emit("stop");
    }

    class StatWatcher extends EventEmitter {
      _handle;
      constructor(path, options) {
        super();
        this._handle = fs.watchFile(path, options, this.#onChange.bind(this));
      }
      #onChange(curr, prev) {
        this.emit("change", curr, prev);
      }
      start() {}
      stop() {
        if (!this._handle)
          return;
        process.nextTick(emitStop, this);
        this._handle.close();
        this._handle = null;
      }
      ref() {
        this._handle?.ref();
      }
      unref() {
        this._handle?.unref();
      }
    }
    const statWatchers = new Map;
    function watchFile(filename, options, listener) {
      filename = getValidatedPath(filename);
      if (typeof options === "function") {
        listener = options;
        options = {};
      }
      if (typeof listener !== "function") {
        throw codes.INVALID_ARG_TYPE("listener", "function", listener);
      }
      var stat = statWatchers.get(filename);
      if (!stat) {
        stat = new StatWatcher(filename, options);
        statWatchers.set(filename, stat);
      }
      stat.addListener("change", listener);
      return stat;
    }
    function unwatchFile(filename, listener) {
      filename = getValidatedPath(filename);
      var stat = statWatchers.get(filename);
      if (!stat)
        return throwIfNullBytesInFileName(filename);
      if (listener) {
        stat.removeListener("change", listener);
        if (stat.listenerCount("change") !== 0) {
          return;
        }
      } else {
        stat.removeAllListeners("change");
      }
      stat.stop();
      statWatchers.delete(filename);
    }
    return { watchFile, unwatchFile };

  })();
  // `fs.promises.watch` (`fs.promises.ts` do bun) sobre o mesmo `binding.watch`: um iterador assíncrono que enfileira os
  // eventos do observador.
  function promisesWatch(filename, options = {}) {
    if (filename instanceof URL) {
      throw new TypeError("Watch URLs are not supported yet");
    } else if (Buffer.isBuffer(filename)) {
      filename = filename.toString();
    } else if (typeof filename !== "string") {
      throw codes.INVALID_ARG_TYPE("filename", ["string", "Buffer", "URL"], filename);
    }
    let nextEventResolve = null;
    if (typeof options === "string") options = { encoding: options };
    const queue = [];
    const ignoreMatcher = watchModule.createIgnoreMatcher(options?.ignore);
    const signal = options?.signal;
    validators.validateAbortSignal(signal, "options.signal");
    const makeAbortError = () => host.internal.makeAbortError(undefined, { cause: signal.reason });
    const wake = () => {
      if (!nextEventResolve) return;
      const resolve = nextEventResolve;
      nextEventResolve = null;
      resolve();
    };

    // O Node nunca abre o observador quando o sinal já está abortado (o gerador lança no primeiro `next()`).
    if (signal?.aborted) {
      return {
        [Symbol.asyncIterator]() {
          let closed = false;
          return {
            async next() {
              if (closed) return { value: undefined, done: true };
              closed = true;
              throw makeAbortError();
            },
            return() {
              closed = true;
              return { value: undefined, done: true };
            },
          };
        },
      };
    }

    const watcher = binding.watch(filename, options || {}, (eventType, name) => {
      if (eventType !== "close" && eventType !== "error" && name != null && ignoreMatcher?.(name)) return;
      queue.push({ __proto__: null, eventType, filename: name });
      wake();
    });

    function onAbort() {
      watcher.close();
      wake();
    }
    signal?.addEventListener("abort", onAbort, { once: true });
    const removeAbortListener = () => signal?.removeEventListener("abort", onAbort);

    return {
      [Symbol.asyncIterator]() {
        let closed = false;
        return {
          async next() {
            while (!closed) {
              if (signal?.aborted) {
                closed = true;
                throw makeAbortError();
              }
              let event;
              while ((event = queue.shift())) {
                if (event.eventType === "close") {
                  closed = true;
                  removeAbortListener();
                  return { value: undefined, done: true };
                }
                if (event.eventType === "error") {
                  closed = true;
                  watcher.close();
                  removeAbortListener();
                  throw event.filename;
                }
                return { value: event, done: false };
              }
              const { promise, resolve } = Promise.withResolvers();
              nextEventResolve = resolve;
              await promise;
            }
            return { value: undefined, done: true };
          },

          return() {
            if (!closed) {
              watcher.close();
              closed = true;
              removeAbortListener();
              wake();
            }
            return { value: undefined, done: true };
          },
        };
      },
    };
  }

  return { watch: watchModule.watch, watchFile: watchFileModule.watchFile, unwatchFile: watchFileModule.unwatchFile, promisesWatch };
})
