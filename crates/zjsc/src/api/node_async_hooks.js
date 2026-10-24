// O módulo `node:async_hooks` do bun 1.4.2, portado de `src/js/node/async_hooks.ts` (MIT, o mesmo texto que o bun
// embute) para JavaScript comum. Roda como o programa `node:async_hooks`, com o corpo estrito, como o dos módulos
// embutidos do bun.
//
// O contexto assíncrono é o `$asyncContext` do JSC do bun: `host.getAsyncContext()` e `host.setAsyncContext(frame)` leem
// e gravam o primeiro campo dele, e `host.setAsyncHooksEnabled(true)` liga o rastreamento (`jsSetAsyncHooksEnabled`).
// Promessas, `queueMicrotask`, `process.nextTick` e os timers capturam o contexto no agendamento e o reinstalam ao
// rodar, como o `AsyncContextFrame` do bun. As dependências internas do bun (`internal/validators`, os `$ERR_*`) vêm de
// `node_internal.js`, com as mensagens e os códigos medidos no bun 1.4.2. O `createHook` só entrega `init` aos
// recursos `TickObject` no bun; o `nextTick` do porte ainda não chama esses ganchos (ver a DIVERGÊNCIA no relato).
//
// `host` traz `throwNodeError(tipo, código, mensagem)`, `inspect(valor, opções)`, `requireBuiltin(nome)`,
// `getAsyncContext()`, `setAsyncContext(frame)` e `setAsyncHooksEnabled(ligado)`.
(function (host) {
  "use strict";

  const ReflectApply = Reflect.apply;
  const NumberIsSafeInteger = Number.isSafeInteger;
  const ObjectDefineProperties = Object.defineProperties;
  const SymbolDispose = Symbol.dispose;
  const ArrayPrototypePush = Array.prototype.push;
  const ArrayPrototypeIndexOf = Array.prototype.indexOf;
  const ArrayPrototypeSplice = Array.prototype.splice;
  const getAsyncContext = host.getAsyncContext;
  const setAsyncContext = host.setAsyncContext;
  const setAsyncHooksEnabled = host.setAsyncHooksEnabled;
  const NodeProcess = process;

  const { codes, validators, makeError } = host.internal;
  const { validateFunction, validateString, validateObject } = validators;

  function arrayPush(array, value) {
    ReflectApply(ArrayPrototypePush, array, [value]);
  }

  // SameValue in pure operators. Node compares stores with the primordial
  // ObjectIs; capturing Object.is here would still inherit a patch applied
  // before this module was lazily loaded.
  function sameValue(a, b) {
    if (a === b) return a !== 0 || 1 / a === 1 / b;
    return a !== a && b !== b;
  }

  class Frame {
    constructor(storage, value, prev, masked) {
      this.storage = storage;
      this.value = value;
      this.prev = prev;
      this.masked = masked;
    }
  }

  // Bumped whenever disable() masks a frame in place, so run() can tell that the
  // chain it installed changed under it even though its identity did not.
  let frameMutations = 0;

  function get() {
    return getAsyncContext();
  }

  function set(frame) {
    return setAsyncContext(frame);
  }

  function isMasked(frame, storage) {
    if (frame === undefined || frame.masked === undefined) return false;
    var masked = frame.masked;
    for (var i = 0, n = masked.length; i < n; i++) {
      if (masked[i] === storage) return true;
    }
    return false;
  }

  function unmask(masked, storage) {
    if (masked === undefined) return undefined;
    var rest = [];
    for (var i = 0, n = masked.length; i < n; i++) {
      if (masked[i] !== storage) arrayPush(rest, masked[i]);
    }
    return rest.length === 0 ? undefined : rest.length === masked.length ? masked : rest;
  }

  // The binding of `storage` visible from `start`, or undefined.
  function lookup(start, storage) {
    if (isMasked(start, storage)) return undefined;
    return find(start, storage);
  }

  // The innermost frame binding `storage` at or below `frame`, ignoring masks.
  function find(frame, storage) {
    for (var f = frame; f !== undefined; f = f.prev) {
      if (f.storage === storage) return f;
    }
    return undefined;
  }

  // A new binding on top of `head`; what was visible from `head` stays visible.
  function push(head, storage, value) {
    return new Frame(storage, value, head, head === undefined ? undefined : unmask(head.masked, storage));
  }

  // `frame` with the binding of `storage` removed. Frames above it are copied
  // (they may be shared with other captures); the tail below it is shared.
  function without(frame, storage) {
    var found = find(frame, storage);
    if (found === undefined) return frame;
    return copyUntil(frame, found, found.prev);
  }

  // Copies [from, stop) onto tail so that the view from the result is the view
  // from `from` minus what was cut out.
  function copyUntil(from, stop, tail) {
    if (from === stop) {
      if (tail === undefined || tail.masked === from.masked) return tail;
      return new Frame(tail.storage, tail.value, tail.prev, from.masked);
    }
    var copied = [];
    for (var f = from; f !== stop; f = f.prev) {
      arrayPush(copied, f);
    }
    for (var i = copied.length - 1; i >= 0; i--) {
      tail = new Frame(copied[i].storage, copied[i].value, tail, copied[i].masked);
    }
    return tail;
  }

  function mergeMasks(a, b) {
    if (a === undefined) return b;
    var merged = [];
    for (var i = 0, n = a.length; i < n; i++) arrayPush(merged, a[i]);
    for (var i = 0, n = b.length; i < n; i++) arrayPush(merged, b[i]);
    return merged;
  }

  // Node parity: dispose() is enterWith(previousStore), which on a fresh ALS
  // leaves a binding to undefined rather than removing it like run().
  class RunScope {
    #storage;
    #previousStore;
    #disposed = false;

    constructor(storage, store) {
      this.#storage = storage;
      this.#previousStore = storage.getStore();
      storage.enterWith(store);
    }

    dispose() {
      if (this.#disposed) {
        return;
      }
      this.#disposed = true;
      this.#storage.enterWith(this.#previousStore);
    }

    [SymbolDispose]() {
      this.dispose();
    }
  }

  class AsyncLocalStorage {
    #defaultValue = undefined;
    #name = undefined;

    constructor(options) {
      if (options !== undefined) {
        validateObject(options, "options");
        this.#defaultValue = options.defaultValue;
        const name = options.name;
        if (name !== undefined) {
          this.#name = `${name}`;
        }
      }
      setAsyncHooksEnabled(true);
    }

    static bind(fn, ...args) {
      validateFunction(fn);
      return this.snapshot().bind(null, fn, ...args);
    }

    static snapshot() {
      var context = get();
      return (fn, ...args) => {
        var prev = get();
        set(context);
        try {
          return ReflectApply(fn, undefined, args);
        } finally {
          set(prev);
        }
      };
    }

    enterWith(store) {
      // Replace rather than shadow an existing binding so repeated enterWith() calls
      // keep the chain bounded by the number of storages.
      set(push(without(get(), this), this, store));
    }

    exit(cb, ...args) {
      return this.run(undefined, cb, ...args);
    }

    run(store_value, callback, ...args) {
      var prior = get();
      var bound = find(prior, this);
      var beforeValue = bound !== undefined && !isMasked(prior, this) ? bound.value : this.#defaultValue;
      // Node short-circuits when the value is unchanged: no enterWith, no
      // finally-restore.
      if (sameValue(beforeValue, store_value)) {
        return ReflectApply(callback, undefined, args);
      }
      var mutations = frameMutations;
      var frame = push(bound === undefined ? prior : copyUntil(prior, bound, bound.prev), this, store_value);
      set(frame);
      try {
        return ReflectApply(callback, undefined, args);
      } finally {
        var head = get();
        if (
          mutations === frameMutations &&
          (head === frame ||
            (head !== undefined && head.prev === frame.prev && head.storage === this && head.masked === frame.masked))
        ) {
          set(prior === undefined ? undefined : new Frame(prior.storage, prior.value, prior.prev, prior.masked));
        } else {
          set(push(without(head, this), this, beforeValue));
        }
      }
    }

    disable() {
      var top = get();
      if (top !== undefined && lookup(top, this) !== undefined) {
        top.masked = mergeMasks(top.masked, [this]);
        frameMutations++;
      }
    }

    get name() {
      return this.#name || "";
    }

    getStore() {
      var start = get();
      if (start === undefined || isMasked(start, this)) return this.#defaultValue;
      for (var f = start; f !== undefined; f = f.prev) {
        if (f.storage === this) return f.value;
      }
      return this.#defaultValue;
    }

    withScope(store) {
      return new RunScope(this, store);
    }

    // Node.js internal function. In Bun's implementation, calling this is not
    // observable from outside the AsyncLocalStorage implementation.
    _enable() {}

    // Node.js internal function. In Bun's implementation, calling this is not
    // observable from outside the AsyncLocalStorage implementation.
    _propagate(_resource, _triggerResource, _type) {}
  }

  class AsyncResource {
    type;
    #snapshot;
    #triggerAsyncId;

    constructor(type, opts) {
      validateString(type, "type");

      // Node defaults to getDefaultTriggerAsyncId() (the current execution async
      // id); Bun does not track async ids, so its executionAsyncId() is 0.
      let triggerAsyncId = typeof opts === "number" ? opts : opts?.triggerAsyncId === undefined ? 0 : opts.triggerAsyncId;
      if (!NumberIsSafeInteger(triggerAsyncId) || triggerAsyncId < -1) {
        throw codes.INVALID_ASYNC_ID("triggerAsyncId", triggerAsyncId);
      }
      if (hasEnabledCreateHook && type.length === 0) {
        throw codes.ASYNC_TYPE(type);
      }

      setAsyncHooksEnabled(true);
      this.type = type;
      this.#snapshot = get();
      this.#triggerAsyncId = triggerAsyncId;
    }

    emitBefore() {
      return true;
    }

    emitAfter() {
      return true;
    }

    asyncId() {
      return 0;
    }

    triggerAsyncId() {
      return this.#triggerAsyncId;
    }

    emitDestroy() {
      //
    }

    runInAsyncScope(fn, thisArg, ...args) {
      var prev = get();
      set(this.#snapshot);
      try {
        return ReflectApply(fn, thisArg, args);
      } finally {
        set(prev);
      }
    }

    bind(fn, thisArg) {
      validateFunction(fn, "fn");
      let bound;
      if (thisArg === undefined) {
        const resource = this;
        bound = function (...args) {
          return resource.runInAsyncScope(fn, this, ...args);
        };
      } else {
        bound = this.runInAsyncScope.bind(this, fn, thisArg);
      }
      ObjectDefineProperties(bound, {
        length: {
          __proto__: null,
          configurable: true,
          enumerable: false,
          value: fn.length,
          writable: false,
        },
      });
      return bound;
    }

    static bind(fn, type, thisArg) {
      type = type || fn.name;
      return new AsyncResource(type || "bound-anonymous-fn").bind(fn, thisArg);
    }
  }

  // The rest of async_hooks is not implemented and is stubbed with no-ops and warnings.

  function createWarning(message, isCreateHook) {
    let warned = false;
    var wrapped = function (arg1) {
      if (warned || (!NodeProcess.env.BUN_FEATURE_FLAG_VERBOSE_WARNINGS && (warned = true))) return;

      const known_supported_modules = [
        // the following do not actually need async_hooks to work properly
        "zx/build/core.js",
        "datadog-core/src/storage/async_resource.js",
      ];
      const e = new Error().stack;
      if (known_supported_modules.some(m => e.includes(m))) return;
      if (isCreateHook && arg1) {
        if (typeof arg1 === "object") {
          const { init, promiseResolve, destroy } = arg1;
          if (init && promiseResolve && destroy) {
            if (isEmptyFunction(init) && isEmptyFunction(destroy)) return;
          }
        }
      }

      warned = true;
      console.warn("[bun] Warning:", message);
    };
    return wrapped;
  }

  function isEmptyFunction(f) {
    let str = f.toString();
    if (!str.startsWith("function()")) return false;
    str = str.slice("function()".length).trim();
    return /^{\s*}$/.test(str);
  }

  const createHookNotImpl = createWarning(
    "async_hooks.createHook is not implemented in Bun. Hooks can still be created but will never be called.",
    true,
  );

  // `internal/async_hooks_tick` e `internal/async_hooks` do bun: a lista de ganchos `init` dos `TickObject` e a
  // contagem de ganchos ligados.
  const tickInitHooks = [];
  let enabledHookCount = 0;

  let hasEnabledCreateHook = false;
  const kHookEnabled = Symbol("kHookEnabled");
  function createHook(hook) {
    validateObject(hook, "hook");
    const { init, before, after, destroy, promiseResolve } = hook;
    if (init !== undefined && typeof init !== "function") throw codes.ASYNC_CALLBACK("hook.init");
    if (before !== undefined && typeof before !== "function") throw codes.ASYNC_CALLBACK("hook.before");
    if (after !== undefined && typeof after !== "function") throw codes.ASYNC_CALLBACK("hook.after");
    if (destroy !== undefined && typeof destroy !== "function") throw codes.ASYNC_CALLBACK("hook.destroy");
    if (promiseResolve !== undefined && typeof promiseResolve !== "function")
      throw codes.ASYNC_CALLBACK("hook.promiseResolve");

    let enabledInit;
    return {
      enable() {
        if (init !== undefined && enabledInit === undefined) {
          // init is delivered for TickObject resources (process.nextTick);
          // other resource types are still unimplemented.
          enabledInit = (asyncId, type, triggerAsyncId, resource) => init(asyncId, type, triggerAsyncId, resource);
          arrayPush(tickInitHooks, enabledInit);
        }
        if (before !== undefined || after !== undefined || destroy !== undefined || promiseResolve !== undefined) {
          createHookNotImpl(hook);
        }
        hasEnabledCreateHook = true;
        if (!this[kHookEnabled]) {
          this[kHookEnabled] = true;
          enabledHookCount++;
        }
        return this;
      },
      disable() {
        if (enabledInit !== undefined) {
          const idx = ReflectApply(ArrayPrototypeIndexOf, tickInitHooks, [enabledInit]);
          if (idx !== -1) ReflectApply(ArrayPrototypeSplice, tickInitHooks, [idx, 1]);
          enabledInit = undefined;
        }
        if (this[kHookEnabled]) {
          this[kHookEnabled] = false;
          enabledHookCount--;
        }
        return this;
      },
    };
  }

  const executionAsyncIdNotImpl = createWarning(
    "async_hooks.executionAsyncId/triggerAsyncId are not implemented in Bun. It will return 0 every time.",
  );
  function executionAsyncId() {
    executionAsyncIdNotImpl();
    return 0;
  }

  function triggerAsyncId() {
    return 0;
  }

  const executionAsyncResourceWarning = createWarning(
    "async_hooks.executionAsyncResource is not implemented in Bun. It returns a reference to process.stdin every time.",
  );
  function executionAsyncResource() {
    executionAsyncResourceWarning();
    return NodeProcess.stdin;
  }

  const asyncWrapProviders = {
    NONE: 0,
    DIRHANDLE: 1,
    DNSCHANNEL: 2,
    ELDHISTOGRAM: 3,
    FILEHANDLE: 4,
    FILEHANDLECLOSEREQ: 5,
    FIXEDSIZEBLOBCOPY: 6,
    FSEVENTWRAP: 7,
    FSREQCALLBACK: 8,
    FSREQPROMISE: 9,
    GETADDRINFOREQWRAP: 10,
    GETNAMEINFOREQWRAP: 11,
    HEAPSNAPSHOT: 12,
    HTTP2SESSION: 13,
    HTTP2STREAM: 14,
    HTTP2PING: 15,
    HTTP2SETTINGS: 16,
    HTTPINCOMINGMESSAGE: 17,
    HTTPCLIENTREQUEST: 18,
    JSSTREAM: 19,
    JSUDPWRAP: 20,
    MESSAGEPORT: 21,
    PIPECONNECTWRAP: 22,
    PIPESERVERWRAP: 23,
    PIPEWRAP: 24,
    PROCESSWRAP: 25,
    PROMISE: 26,
    QUERYWRAP: 27,
    SHUTDOWNWRAP: 28,
    SIGNALWRAP: 29,
    STATWATCHER: 30,
    STREAMPIPE: 31,
    TCPCONNECTWRAP: 32,
    TCPSERVERWRAP: 33,
    TCPWRAP: 34,
    TTYWRAP: 35,
    UDPSENDWRAP: 36,
    UDPWRAP: 37,
    SIGINTWATCHDOG: 38,
    WORKER: 39,
    WORKERHEAPSNAPSHOT: 40,
    WRITEWRAP: 41,
    ZLIB: 42,
    CHECKPRIMEREQUEST: 43,
    PBKDF2REQUEST: 44,
    KEYPAIRGENREQUEST: 45,
    KEYGENREQUEST: 46,
    KEYEXPORTREQUEST: 47,
    CIPHERREQUEST: 48,
    DERIVEBITSREQUEST: 49,
    HASHREQUEST: 50,
    RANDOMBYTESREQUEST: 51,
    RANDOMPRIMEREQUEST: 52,
    SCRYPTREQUEST: 53,
    SIGNREQUEST: 54,
    TLSWRAP: 55,
    VERIFYREQUEST: 56,
    INSPECTORJSBINDING: 57,
  };

  return {
    AsyncLocalStorage,
    createHook,
    executionAsyncId,
    triggerAsyncId,
    executionAsyncResource,
    asyncWrapProviders,
    AsyncResource,
  };
});
