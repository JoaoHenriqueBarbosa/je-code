// O módulo `node:events` do bun 1.4.2, portado de `src/js/node/events.ts` (MIT, o mesmo texto que o bun embute) para
// JavaScript comum. Roda como o programa `node:events`, com o corpo estrito, como o dos módulos embutidos do bun.
//
// O que o bun escreve com intrínsecos do JSC (`$call`, `$apply`, `$newArrayWithSize`, `$ERR_*`, `$debug`, ...) é
// trocado pelo equivalente público capturado aqui, antes do programa mexer nos protótipos, e as dependências internas
// do bun (`internal/validators`, `internal/abort_listener`, `internal/fixed_queue`, `internal/shared`) entram como as
// funções locais mais abaixo, com as mensagens e os códigos medidos no bun 1.4.2.
//
// `host` traz as funções nativas: `throwNodeError(tipo, código, mensagem)`, `inspect(valor, opções)`,
// `isEventTarget(valor)`, `eventTargetListeners(alvo, tipo)` e `requireBuiltin(nome)`.
(function (host) {
  "use strict";

  const ReflectApply = Reflect.apply;
  const ReflectOwnKeys = Reflect.ownKeys;
  const ObjectCreate = Object.create;
  const ObjectDefineProperty = Object.defineProperty;
  const ArrayPrototypeUnshift = Array.prototype.unshift;
  const PromiseResolve = Promise.resolve.bind(Promise);
  const PromiseReject = Promise.reject.bind(Promise);
  const SymbolFor = Symbol.for;
  const SymbolDispose = Symbol.dispose;
  const SymbolAsyncIterator = Symbol.asyncIterator;
  const NodeProcess = process;

  // ---- erros com código, no formato do `node:internal/errors` do bun ----

  // Erros, validadores e `addAbortListener` são os de `node_internal.js`, que o `stream` também usa.
  const {
    codes,
    makeAbortError,
    resistStopPropagation,
    addAbortListener,
    validators: { validateObject, validateInteger, validateAbortSignal, validateNumber, validateBoolean, validateFunction, validateString },
  } = host.internal;
  const invalidArgType = codes.INVALID_ARG_TYPE;

  // `internal/fixed_queue`, só o que `on` usa.
  class FixedQueue {
    constructor() {
      this.items = [];
      this.head = 0;
    }
    isEmpty() {
      return this.head === this.items.length;
    }
    push(value) {
      this.items.push(value);
    }
    shift() {
      if (this.head === this.items.length) return undefined;
      const value = this.items[this.head];
      this.items[this.head++] = undefined;
      if (this.head === this.items.length) {
        this.items.length = 0;
        this.head = 0;
      }
      return value;
    }
  }

  // ---- o módulo ----

  const kCapture = Symbol("kCapture");
  // Set when `_events` was preallocated (streams do this): removeListener then writes `undefined` instead of
  // `delete`, keeping one shared JSC Structure.
  const kShapeMode = Symbol("shapeMode");
  const kErrorMonitor = SymbolFor("events.errorMonitor");
  const kMaxEventTargetListeners = Symbol("events.maxEventTargetListeners");
  const kMaxEventTargetListenersWarned = Symbol("events.maxEventTargetListenersWarned");
  const kWatermarkData = SymbolFor("nodejs.watermarkData");
  const kRejection = SymbolFor("nodejs.rejection");
  const kFirstEventParam = SymbolFor("nodejs.kFirstEventParam");
  const captureRejectionSymbol = SymbolFor("nodejs.rejection");

  const { kEmptyObject } = host.internal;

  var defaultMaxListeners = 10;

  // EventEmitter must be a standard function because some old code will do weird tricks like
  // `EventEmitter.apply(this)`.
  function EventEmitter(opts) {
    if (this._events === undefined || this._events === this.__proto__._events) {
      this._events = ObjectCreate(null);
      this._eventsCount = 0;
      this[kShapeMode] = false;
    } else {
      // Preallocated `_events` (streams). The count comes from the prototype default
      // `EventEmitterPrototype._eventsCount = 0`, as in node.
      this[kShapeMode] = true;
    }

    this._maxListeners ??= undefined;
    if (opts?.captureRejections) {
      validateBoolean(opts.captureRejections, "options.captureRejections");
      this[kCapture] = !!opts.captureRejections;
      this.emit = emitWithRejectionCapture;
    } else {
      this[kCapture] = EventEmitterPrototype[kCapture];
      const capture = EventEmitterPrototype[kCapture];
      this[kCapture] = capture;
      if (capture) {
        this.emit = emitWithRejectionCapture;
      }
    }
  }
  ObjectDefineProperty(EventEmitter, "name", { value: "EventEmitter", configurable: true });
  const EventEmitterPrototype = (EventEmitter.prototype = {});

  EventEmitterPrototype.setMaxListeners = function setMaxListeners(n) {
    validateNumber(n, "setMaxListeners", 0);
    this._maxListeners = n;
    return this;
  };
  ObjectDefineProperty(EventEmitterPrototype.setMaxListeners, "name", { value: "setMaxListeners" });

  EventEmitterPrototype.constructor = EventEmitter;

  EventEmitterPrototype.getMaxListeners = function getMaxListeners() {
    return _getMaxListeners(this);
  };
  ObjectDefineProperty(EventEmitterPrototype.getMaxListeners, "name", { value: "getMaxListeners" });

  function emitError(emitter, args) {
    var { _events: events } = emitter;

    if (events !== undefined) {
      const errorMonitor = events[kErrorMonitor];
      if (errorMonitor !== undefined) {
        applyHandlers(errorMonitor, emitter, args);
      }

      const handlers = events.error;
      if (handlers !== undefined) {
        applyHandlers(handlers, emitter, args);
        return true;
      }
    }

    let er;
    if (args.length > 0) er = args[0];

    if (Error.isError(er)) {
      throw er; // Unhandled 'error' event
    }

    let stringifiedEr;
    try {
      stringifiedEr = host.inspect(er, undefined);
    } catch {
      stringifiedEr = er;
    }

    // At least give some kind of context to the user
    const err = codes.UNHANDLED_ERROR(stringifiedEr);
    err.context = er;
    throw err; // Unhandled 'error' event
  }

  // A listener list is a bare function for a single listener, else an array (like node). Arrays are never mutated in
  // place - mutators install a copy - so a stored list can be iterated with no defensive clone.
  function applyHandlers(handlers, emitter, args) {
    if (typeof handlers === "function") {
      ReflectApply(handlers, emitter, args);
      return;
    }
    for (let i = 0, { length } = handlers; i < length; i++) {
      ReflectApply(handlers[i], emitter, args);
    }
  }

  function addCatch(emitter, promise, type, args) {
    promise.then(undefined, function (err) {
      // The callback is called with nextTick to avoid a follow-up rejection from this promise.
      NodeProcess.nextTick(emitUnhandledRejectionOrErr, emitter, err, type, args);
    });
  }

  function emitUnhandledRejectionOrErr(emitter, err, type, args) {
    if (typeof emitter[kRejection] === "function") {
      emitter[kRejection](err, type, ...args);
    } else {
      // If the error handler throws, it is not catchable and it will end up in 'uncaughtException'.
      // We restore the previous value of kCapture in case the uncaughtException is present
      // and the exception is handled.
      try {
        emitter[kCapture] = false;
        emitter.emit("error", err);
      } finally {
        emitter[kCapture] = true;
      }
    }
  }

  const emitWithoutRejectionCapture = function emit(type, ...args) {
    if (type === "error") {
      return emitError(this, args);
    }
    var { _events: events } = this;
    if (events === undefined) return false;
    var handler = events[type];
    if (handler === undefined) return false;
    if (typeof handler === "function") {
      ReflectApply(handler, this, args);
      return true;
    }
    // No defensive clone: stored arrays are never mutated in place (mutators install a copy), so this list stays
    // stable for the whole loop even if a listener adds/removes listeners.
    for (let i = 0, { length } = handler; i < length; i++) {
      ReflectApply(handler[i], this, args);
    }
    return true;
  };

  const emitWithRejectionCapture = function emit(type, ...args) {
    if (type === "error") {
      return emitError(this, args);
    }
    var { _events: events } = this;
    if (events === undefined) return false;
    var handler = events[type];
    if (handler === undefined) return false;
    if (typeof handler === "function") {
      const result = ReflectApply(handler, this, args);
      if (result !== undefined && result instanceof Promise) {
        addCatch(this, result, type, args);
      }
      return true;
    }
    for (let i = 0, { length } = handler; i < length; i++) {
      const result = ReflectApply(handler[i], this, args);
      if (result !== undefined && result instanceof Promise) {
        addCatch(this, result, type, args);
      }
    }
    return true;
  };

  EventEmitterPrototype.emit = emitWithoutRejectionCapture;

  function _addListener(target, type, fn, prepend) {
    checkListener(fn);
    var events = target._events;
    if (!events) {
      events = target._events = ObjectCreate(null);
      target._eventsCount = 0;
    } else if (events.newListener) {
      target.emit("newListener", type, fn.listener ?? fn);
      // A newListener handler can replace `_events` (e.g. a once wrapper's removeListener dropping the count to 0
      // installs a fresh object).
      events = target._events;
    }
    var existing = events[type];
    if (existing === undefined) {
      // A single listener is stored bare, like node, so this allocates nothing.
      events[type] = fn;
      target._eventsCount++;
      return;
    }
    var handlers;
    if (typeof existing === "function") {
      handlers = events[type] = prepend ? [fn, existing] : [existing, fn];
    } else {
      handlers = events[type] = copyWithInserted(existing, fn, prepend);
    }
    var m = _getMaxListeners(target);
    if (m > 0 && handlers.length > m && !handlers.warned) {
      overflowWarning(target, type, handlers);
    }
  }

  EventEmitterPrototype.addListener = function addListener(type, fn) {
    _addListener(this, type, fn, false);
    return this;
  };

  EventEmitterPrototype.on = EventEmitterPrototype.addListener;

  EventEmitterPrototype.prependListener = function prependListener(type, fn) {
    _addListener(this, type, fn, true);
    return this;
  };

  // Copy-on-write: emit iterates stored arrays with no clone, so new listeners land in a fresh array; `warned`
  // carries over so the leak warning fires once.
  function copyWithInserted(list, fn, prepend) {
    const n = list.length;
    const copy = new Array(n + 1);
    if (prepend) {
      copy[0] = fn;
      for (let i = 0; i < n; i++) copy[i + 1] = list[i];
    } else {
      for (let i = 0; i < n; i++) copy[i] = list[i];
      copy[n] = fn;
    }
    if (list.warned) copy.warned = true;
    return copy;
  }

  function overflowWarning(emitter, type, handlers) {
    handlers.warned = true;
    const warn = new Error(
      `Possible EventEmitter memory leak detected. ${handlers.length} ${String(type)} listeners added to ${host.inspect(emitter, { depth: -1 })}. MaxListeners is ${_getMaxListeners(emitter)}. Use emitter.setMaxListeners() to increase limit`,
    );
    warn.name = "MaxListenersExceededWarning";
    warn.emitter = emitter;
    warn.type = type;
    warn.count = handlers.length;
    NodeProcess.emitWarning(warn);
  }

  // A closure over (target, type, listener, fired) rather than a state object plus onceWrapper.bind(state): one
  // allocation instead of two per once().
  function _onceWrap(target, type, listener) {
    let fired = false;
    // Named `onceWrapper` so inspect/rawListeners() output tracks node's.
    const wrapped = function onceWrapper() {
      if (!fired) {
        fired = true;
        // Drop closure refs so anything that retains the fired wrapper does not retain the emitter.
        // `wrapped.listener` stays: node asserts it survives.
        const t = target;
        const l = listener;
        target = undefined;
        listener = undefined;
        t.removeListener(type, wrapped);
        if (arguments.length === 0) return ReflectApply(l, t, []);
        return ReflectApply(l, t, arguments);
      }
    };
    wrapped.listener = listener;
    return wrapped;
  }

  EventEmitterPrototype.once = function once(type, fn) {
    checkListener(fn);
    this.on(type, _onceWrap(this, type, fn));
    return this;
  };
  ObjectDefineProperty(EventEmitterPrototype.once, "name", { value: "once" });

  EventEmitterPrototype.prependOnceListener = function prependOnceListener(type, fn) {
    checkListener(fn);
    this.prependListener(type, _onceWrap(this, type, fn));
    return this;
  };

  EventEmitterPrototype.removeListener = function removeListener(type, listener) {
    checkListener(listener);

    const events = this._events;
    if (events === undefined) return this;

    const list = events[type];
    if (list === undefined) return this;

    if (typeof list === "function") {
      // Bare single listener.
      if (list !== listener && list.listener !== listener) return this;
      this._eventsCount--;
      if (this[kShapeMode]) {
        // Keep the preallocated slot; just clear it.
        events[type] = undefined;
      } else if (this._eventsCount === 0) {
        // Fresh object: drops any add/delete transition history.
        this._events = ObjectCreate(null);
      } else {
        delete events[type];
      }
      if (events.removeListener !== undefined) this.emit("removeListener", type, list.listener ?? listener);
      return this;
    }

    let position = -1;
    for (let i = list.length - 1; i >= 0; i--) {
      if (list[i] === listener || list[i].listener === listener) {
        position = i;
        break;
      }
    }
    if (position < 0) return this;

    // Copy-remove (arrays are never mutated in place), and store a lone survivor bare like node does.
    const n = list.length;
    const copy = new Array(n - 1);
    for (let i = 0, j = 0; i < n; i++) {
      if (i !== position) copy[j++] = list[i];
    }
    if (list.warned) copy.warned = true;
    events[type] = copy.length === 1 ? copy[0] : copy;

    if (events.removeListener !== undefined) this.emit("removeListener", type, listener.listener ?? listener);

    return this;
  };

  EventEmitterPrototype.off = EventEmitterPrototype.removeListener;

  EventEmitterPrototype.removeAllListeners = function removeAllListeners(type) {
    const events = this._events;
    if (events === undefined) return this;

    // Not listening for removeListener, no need to emit
    if (events.removeListener === undefined) {
      if (arguments.length === 0) {
        this._events = ObjectCreate(null);
        this._eventsCount = 0;
      } else if (events[type] !== undefined) {
        if (--this._eventsCount === 0) this._events = ObjectCreate(null);
        else delete events[type];
      }
      this[kShapeMode] = false;
      return this;
    }

    // Emit removeListener for all listeners on all events
    if (arguments.length === 0) {
      for (const key of ReflectOwnKeys(events)) {
        if (key === "removeListener") continue;
        this.removeAllListeners(key);
      }
      this.removeAllListeners("removeListener");
      this._events = ObjectCreate(null);
      this._eventsCount = 0;
      this[kShapeMode] = false;
      return this;
    }

    const listeners = events[type];
    if (typeof listeners === "function") {
      this.removeListener(type, listeners);
    } else if (listeners !== undefined) {
      // LIFO order. `listeners` is our own snapshot; each removeListener call installs a fresh array (or bare fn /
      // nothing), so it stays intact here.
      for (let i = listeners.length - 1; i >= 0; i--) this.removeListener(type, listeners[i]);
    }
    return this;
  };

  EventEmitterPrototype.listeners = function listeners(type) {
    var { _events: events } = this;
    if (!events) return [];
    var handlers = events[type];
    if (!handlers) return [];
    if (typeof handlers === "function") return [handlers.listener ?? handlers];
    return handlers.map(x => x.listener ?? x);
  };

  EventEmitterPrototype.rawListeners = function rawListeners(type) {
    var { _events } = this;
    if (!_events) return [];
    var handlers = _events[type];
    if (!handlers) return [];
    if (typeof handlers === "function") return [handlers];
    return handlers.slice();
  };

  EventEmitterPrototype.listenerCount = function listenerCount(type, method) {
    var handlers = this._events?.[type];
    if (handlers === undefined) return 0;
    if (typeof handlers === "function") {
      if (method != null) return handlers === method || handlers.listener === method ? 1 : 0;
      return 1;
    }
    if (method != null) {
      var length = 0;
      for (let i = 0; i < handlers.length; i++) {
        const handler = handlers[i];
        if (handler === method || handler.listener === method) {
          length++;
        }
      }
      return length;
    }
    return handlers.length;
  };
  ObjectDefineProperty(EventEmitterPrototype.listenerCount, "name", { value: "listenerCount" });

  EventEmitterPrototype.eventNames = function eventNames() {
    return this._eventsCount > 0 ? Reflect.ownKeys(this._events) : [];
  };

  EventEmitterPrototype[kCapture] = false;
  // Prototype default, like node: the shape-mode constructor branch (a preallocated _events object, i.e. every
  // stream) skips the own-property init, so without this the count is undefined and every ++/-- yields NaN.
  EventEmitterPrototype._eventsCount = 0;

  // `async` so the validation/already-aborted `throw`s below surface as a rejected promise instead of a synchronous
  // throw: matches Node, whose `once` is also an async function.
  async function once(emitter, type, options = kEmptyObject) {
    validateObject(options, "options");
    var signal = options?.signal;
    validateAbortSignal(signal, "options.signal");
    if (signal?.aborted) {
      throw makeAbortError(undefined, { cause: signal?.reason });
    }
    let settle;
    const promise = new Promise((resolve, reject) => {
      settle = { resolve, reject };
    });
    const errorListener = err => {
      emitter.removeListener(type, resolver);
      if (signal != null) {
        eventTargetAgnosticRemoveListener(signal, "abort", abortListener);
      }
      settle.reject(err);
    };
    const resolver = (...args) => {
      if (typeof emitter.removeListener === "function") {
        emitter.removeListener("error", errorListener);
      }
      if (signal != null) {
        eventTargetAgnosticRemoveListener(signal, "abort", abortListener);
      }
      settle.resolve(args);
    };
    const opts = resistStopPropagation({ __proto__: null, once: true });
    eventTargetAgnosticAddListener(emitter, type, resolver, opts);
    if (type !== "error" && typeof emitter.once === "function") {
      // EventTarget does not have `error` event semantics like Node EventEmitters, we listen to `error` events only
      // on EventEmitters.
      emitter.once("error", errorListener);
    }
    function abortListener() {
      eventTargetAgnosticRemoveListener(emitter, type, resolver);
      eventTargetAgnosticRemoveListener(emitter, "error", errorListener);
      settle.reject(makeAbortError(undefined, { cause: signal?.reason }));
    }
    if (signal != null) {
      eventTargetAgnosticAddListener(signal, "abort", abortListener, opts);
    }

    return promise;
  }
  ObjectDefineProperty(once, "name", { value: "once" });

  const AsyncIteratorPrototype = Object.getPrototypeOf(Object.getPrototypeOf(async function* () {}).prototype);
  function createIterResult(value, done) {
    return { value, done };
  }
  function on(emitter, event, options = kEmptyObject) {
    // Parameters validation
    validateObject(options, "options");
    const signal = options.signal;
    validateAbortSignal(signal, "options.signal");
    if (signal?.aborted) throw makeAbortError(undefined, { cause: signal?.reason });
    // Support both highWaterMark and highWatermark for backward compatibility
    const highWatermark = options.highWaterMark ?? options.highWatermark ?? Number.MAX_SAFE_INTEGER;
    validateInteger(highWatermark, "options.highWaterMark", 1);
    // Support both lowWaterMark and lowWatermark for backward compatibility
    const lowWatermark = options.lowWaterMark ?? options.lowWatermark ?? 1;
    validateInteger(lowWatermark, "options.lowWaterMark", 1);

    // Preparing controlling queues and variables
    const unconsumedEvents = new FixedQueue();
    const unconsumedPromises = new FixedQueue();
    let paused = false;
    let error = null;
    let finished = false;
    let size = 0;

    const iterator = Object.setPrototypeOf(
      {
        next() {
          // First, we consume all unread events
          if (size) {
            const value = unconsumedEvents.shift();
            size--;
            if (paused && size < lowWatermark) {
              emitter.resume();
              paused = false;
            }
            return PromiseResolve(createIterResult(value, false));
          }

          // Then we error, if an error happened. This happens one time if at all, because after 'error' we stop
          // listening
          if (error) {
            const p = PromiseReject(error);
            // Only the first element errors
            error = null;
            return p;
          }

          // If the iterator is finished, resolve to done
          if (finished) return closeHandler();

          // Wait until an event happens
          return new Promise(function (resolve, reject) {
            unconsumedPromises.push({ resolve, reject });
          });
        },

        return() {
          return closeHandler();
        },

        throw(err) {
          if (!err || !(err instanceof Error)) {
            throw invalidArgType("EventEmitter.AsyncIterator", "Error", err);
          }
          errorHandler(err);
        },
        [SymbolAsyncIterator]() {
          return this;
        },
        [kWatermarkData]: {
          get size() {
            return size;
          },
          get low() {
            return lowWatermark;
          },
          get high() {
            return highWatermark;
          },
          get isPaused() {
            return paused;
          },
        },
      },
      AsyncIteratorPrototype,
    );

    // Adding event handlers
    const { addEventListener, removeAll } = listenersController();
    addEventListener(
      emitter,
      event,
      options[kFirstEventParam]
        ? eventHandler
        : function (...args) {
            return eventHandler(args);
          },
    );
    if (event !== "error" && typeof emitter.on === "function") {
      addEventListener(emitter, "error", errorHandler);
    }
    const closeEvents = options?.close;
    if (closeEvents?.length) {
      for (let i = 0; i < closeEvents.length; i++) {
        addEventListener(emitter, closeEvents[i], closeHandler);
      }
    }

    const abortListenerDisposable = signal ? addAbortListener(signal, abortListener) : null;

    return iterator;

    function abortListener() {
      errorHandler(makeAbortError(undefined, { cause: signal?.reason }));
    }

    function eventHandler(value) {
      if (unconsumedPromises.isEmpty()) {
        size++;
        if (!paused && size > highWatermark) {
          paused = true;
          emitter.pause();
        }
        unconsumedEvents.push(value);
      } else unconsumedPromises.shift().resolve(createIterResult(value, false));
    }

    function errorHandler(err) {
      if (unconsumedPromises.isEmpty()) error = err;
      else unconsumedPromises.shift().reject(err);

      closeHandler();
    }

    function closeHandler() {
      abortListenerDisposable?.[SymbolDispose]();
      removeAll();
      finished = true;
      const doneResult = createIterResult(undefined, true);
      while (!unconsumedPromises.isEmpty()) {
        unconsumedPromises.shift().resolve(doneResult);
      }

      return PromiseResolve(doneResult);
    }
  }
  ObjectDefineProperty(on, "name", { value: "on" });

  function listenersController() {
    const listeners = [];

    return {
      addEventListener(emitter, event, handler, flags) {
        eventTargetAgnosticAddListener(emitter, event, handler, flags);
        listeners.push([emitter, event, handler, flags]);
      },
      removeAll() {
        while (listeners.length > 0) {
          const [emitter, event, handler, flags] = listeners.pop();
          eventTargetAgnosticRemoveListener(emitter, event, handler, flags);
        }
      },
    };
  }

  function getEventListeners(emitter, type) {
    if (typeof emitter?.listeners === "function") {
      return emitter.listeners(type);
    }

    return host.eventTargetListeners(emitter, type);
  }

  // https://github.com/nodejs/node/blob/2eff28fb7a93d3f672f80b582f664a7c701569fb/lib/events.js#L315-L339
  function setMaxListeners(n = defaultMaxListeners, ...eventTargets) {
    validateNumber(n, "setMaxListeners", 0);
    if (eventTargets.length === 0) {
      defaultMaxListeners = n;
    } else {
      for (let i = 0; i < eventTargets.length; i++) {
        const target = eventTargets[i];
        if (host.isEventTarget(target)) {
          target[kMaxEventTargetListeners] = n;
          target[kMaxEventTargetListenersWarned] = false;
        } else if (typeof target.setMaxListeners === "function") {
          target.setMaxListeners(n);
        } else {
          throw invalidArgType("eventTargets", ["EventEmitter", "EventTarget"], target);
        }
      }
    }
  }
  ObjectDefineProperty(setMaxListeners, "name", { value: "setMaxListeners" });

  function listenerCount(emitter, type) {
    if (typeof emitter.listenerCount === "function") {
      return emitter.listenerCount(type);
    }

    // EventTarget
    const count = host.eventTargetListenerCount(emitter, type);
    if (count !== undefined) return count;

    throw invalidArgType("emitter", ["EventEmitter", "EventTarget"], emitter);
  }
  ObjectDefineProperty(listenerCount, "name", { value: "listenerCount" });

  function eventTargetAgnosticRemoveListener(emitter, name, listener, flags) {
    if (typeof emitter.removeListener === "function") {
      emitter.removeListener(name, listener);
    } else if (typeof emitter.removeEventListener === "function") {
      emitter.removeEventListener(name, listener, flags);
    } else {
      throw invalidArgType("emitter", "EventEmitter", emitter);
    }
  }

  function eventTargetAgnosticAddListener(emitter, name, listener, flags) {
    if (typeof emitter.on === "function") {
      if (flags?.once) {
        emitter.once(name, listener);
      } else {
        emitter.on(name, listener);
      }
    } else if (typeof emitter.addEventListener === "function") {
      emitter.addEventListener(name, listener, flags);
    } else {
      throw invalidArgType("emitter", "EventEmitter", emitter);
    }
  }

  function checkListener(listener) {
    validateFunction(listener, "listener");
  }

  function _getMaxListeners(emitter) {
    return emitter?._maxListeners ?? defaultMaxListeners;
  }

  let AsyncResource = null;

  function getMaxListeners(emitterOrTarget) {
    if (typeof emitterOrTarget?.getMaxListeners === "function") {
      return _getMaxListeners(emitterOrTarget);
    } else if (host.isEventTarget(emitterOrTarget)) {
      emitterOrTarget[kMaxEventTargetListeners] ??= defaultMaxListeners;
      return emitterOrTarget[kMaxEventTargetListeners];
    }
    throw invalidArgType("emitter", ["EventEmitter", "EventTarget"], emitterOrTarget);
  }
  ObjectDefineProperty(getMaxListeners, "name", { value: "getMaxListeners" });

  let EventEmitterReferencingAsyncResource;
  function lazyLoadAsyncResource() {
    if (!AsyncResource) {
      AsyncResource = host.requireBuiltin("node:async_hooks").AsyncResource;
      EventEmitterReferencingAsyncResource = class EventEmitterReferencingAsyncResource extends AsyncResource {
        #eventEmitter;

        constructor(ee, type, options) {
          super(type, options);
          this.#eventEmitter = ee;
        }

        get eventEmitter() {
          return this.#eventEmitter;
        }
      };
    }
  }

  class EventEmitterAsyncResource extends EventEmitter {
    #asyncResource;

    constructor(options) {
      lazyLoadAsyncResource();
      let name;
      if (typeof options === "string") {
        name = options;
        options = undefined;
      } else {
        if (new.target === EventEmitterAsyncResource) {
          validateString(options?.name, "options.name");
        }
        name = options?.name || new.target.name;
      }
      super(options);
      this.#asyncResource = new EventEmitterReferencingAsyncResource(this, name, options);
      // EventEmitter's constructor stamps `this.emit = emitWithRejectionCapture` as an OWN property when
      // captureRejections is on, which would shadow the prototype's runInAsyncScope-wrapped emit below. Remove it so
      // listeners still run in the resource's async scope; the prototype emit re-checks this[kCapture] on every call,
      // so rejection capture is preserved.
      delete this.emit;
    }

    get asyncId() {
      return this.#asyncResource.asyncId();
    }

    get triggerAsyncId() {
      return this.#asyncResource.triggerAsyncId();
    }

    get asyncResource() {
      return this.#asyncResource;
    }

    emit(event, ...args) {
      const asyncResource = this.#asyncResource;
      const emit = this[kCapture] ? emitWithRejectionCapture : super.emit;
      ReflectApply(ArrayPrototypeUnshift, args, [emit, this, event]);
      return ReflectApply(asyncResource.runInAsyncScope, asyncResource, args);
    }

    emitDestroy() {
      this.#asyncResource.emitDestroy();
    }
  }

  Object.defineProperties(EventEmitter, {
    captureRejections: {
      get() {
        return EventEmitterPrototype[kCapture];
      },
      set(value) {
        validateBoolean(value, "EventEmitter.captureRejections");

        EventEmitterPrototype[kCapture] = value;
      },
      enumerable: true,
    },
    defaultMaxListeners: {
      enumerable: true,
      get: () => {
        return defaultMaxListeners;
      },
      set: arg => {
        validateNumber(arg, "defaultMaxListeners", 0);
        defaultMaxListeners = arg;
      },
    },
    kMaxEventTargetListeners: {
      value: kMaxEventTargetListeners,
      enumerable: false,
      configurable: false,
      writable: false,
    },
    kMaxEventTargetListenersWarned: {
      value: kMaxEventTargetListenersWarned,
      enumerable: false,
      configurable: false,
      writable: false,
    },
  });
  Object.assign(EventEmitter, {
    once,
    on,
    getEventListeners,
    getMaxListeners,
    setMaxListeners,
    EventEmitter,
    usingDomains: false,
    captureRejectionSymbol,
    EventEmitterAsyncResource,
    errorMonitor: kErrorMonitor,
    addAbortListener,
    init: EventEmitter,
    listenerCount,
  });

  // Node: `Object.getPrototypeOf(process) instanceof EventEmitter` holds. Link the native process prototype under
  // this module's EventEmitter; the native methods stay earlier in the chain and keep winning lookups.
  try {
    const processPrototype = Object.getPrototypeOf(NodeProcess);
    if (
      processPrototype !== null &&
      processPrototype !== Object.prototype &&
      !(processPrototype instanceof EventEmitter)
    ) {
      Object.setPrototypeOf(processPrototype, EventEmitter.prototype);
    }
  } catch {
    // If the prototype is not relinkable, process simply keeps its native chain; everything else about this module
    // still works.
  }

  return EventEmitter;
})
