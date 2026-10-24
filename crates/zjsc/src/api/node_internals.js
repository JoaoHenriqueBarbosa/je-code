// GERADO por `scripts/gen-node-internals.js` a partir dos `src/js/internal/*` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { validators, toClass, codes, makeAbortError } = host.internal;
  const debug = () => {};
  const assert = () => {};
  const isObject = value => value !== null && (typeof value === "object" || typeof value === "function");
  // `$newPromise()` e `$resolvePromise()` do JSC: uma promessa sem executor, resolvida de fora.
  const promiseResolvers = new WeakMap();
  const newPromise = () => {
    let resolve;
    const promise = new Promise(settle => {
      resolve = settle;
    });
    promiseResolvers.set(promise, resolve);
    return promise;
  };
  const resolvePromise = (promise, value) => promiseResolvers.get(promise)(value);
  const isBlob = value => typeof Blob === "function" && value instanceof Blob;
  // O que ainda não existe (TLS, `BlockList`...): lança no ponto de uso, nunca finge.
  const notImplemented = what => () => host.internal.throwNotImplemented(what);
  // O estado de `node_http_binding.rs` do bun.
  let maxHeaderSize = 16384;
  const httpBinding = {
    getMaxHTTPHeaderSize: () => maxHeaderSize,
    setMaxHTTPHeaderSize(value) {
      if (!Number.isInteger(value) || value < 0) throw host.internal.makeError("TypeError", "ERR_INVALID_ARG_TYPE", `The "maxHeaderSize" argument must be of type non-negative integer. Received ${host.internal.determineSpecificType(value)}`);
      maxHeaderSize = value;
    },
  };
  function define_native_secure_context() {
      function SecureContext() {
        return host.internal.throwNotImplemented("tls SecureContext");
      }
      toClass(SecureContext, "SecureContext");
      for (const name of ["intern", "createPrivate", "parsePkcs12"]) SecureContext[name] = () => host.internal.throwNotImplemented("tls SecureContext");
      return SecureContext;
  }
  function define_native_http_parser() {
      const methods = ["DELETE", "GET", "HEAD", "POST", "PUT", "CONNECT", "OPTIONS", "TRACE", "COPY", "LOCK", "MKCOL", "MOVE", "PROPFIND", "PROPPATCH", "SEARCH", "UNLOCK", "BIND", "REBIND", "UNBIND", "ACL", "REPORT", "MKACTIVITY", "CHECKOUT", "MERGE", "M-SEARCH", "NOTIFY", "SUBSCRIBE", "UNSUBSCRIBE", "PATCH", "PURGE", "MKCALENDAR", "LINK", "UNLINK", "SOURCE", "QUERY"];
      const allMethods = [...methods.slice(0, 34), "PRI", "DESCRIBE", "ANNOUNCE", "SETUP", "PLAY", "PAUSE", "TEARDOWN", "GET_PARAMETER", "SET_PARAMETER", "REDIRECT", "RECORD", "FLUSH", "QUERY"];
      function HTTPParser() {
        return host.internal.throwNotImplemented("http parser");
      }
      toClass(HTTPParser, "HTTPParser");
      for (const [name, value] of Object.entries({
        REQUEST: 1, RESPONSE: 2, kOnMessageBegin: 0, kOnHeaders: 1, kOnHeadersComplete: 2, kOnBody: 3, kOnMessageComplete: 4, kOnExecute: 5,
        kOnTimeout: 6, kLenientNone: 0, kLenientHeaders: 1, kLenientChunkedLength: 2, kLenientKeepAlive: 4, kLenientTransferEncoding: 8,
        kLenientVersion: 16, kLenientDataAfterClose: 32, kLenientOptionalLFAfterCR: 64, kLenientOptionalCRLFAfterChunk: 128,
        kLenientOptionalCRBeforeLF: 256, kLenientSpacesAfterChunkSize: 512, kLenientHeaderValueRelaxed: 1, kLenientAll: 1023,
      })) HTTPParser[name] = value;
      for (const name of ["close", "free", "remove", "execute", "finish", "initialize", "pause", "resume", "consume", "unconsume", "getCurrentBuffer", "duration", "headersCompleted"]) {
        HTTPParser.prototype[name] = function () {
          return host.internal.throwNotImplemented("http parser");
        };
      }
      function ConnectionsList() {
        return host.internal.throwNotImplemented("http connections list");
      }
      toClass(ConnectionsList, "ConnectionsList");
      return { methods, allMethods, HTTPParser, ConnectionsList };
  }
  function define_native_native_readable() {
      const Readable = require("internal/streams/readable");
      function constructNativeReadable(readableStream, options) {
        const reader = readableStream.getReader();
        let reading = false;
        return new Readable({
          ...options,
          read() {
            if (reading) return;
            reading = true;
            reader.read().then(
              ({ done, value }) => {
                reading = false;
                if (done) this.push(null);
                else this.push(value);
              },
              error => {
                reading = false;
                this.destroy(error);
              },
            );
          },
          destroy(error, callback) {
            reader.cancel(error).then(() => callback(error), () => callback(error));
          },
        });
      }
      return { constructNativeReadable };
  }
  function define_internal_trace_events() {
      return { isCategoryGroupEnabled: () => false, emitEvent() {}, emitEventWithArgs() {} };
  }
  const cache = { __proto__: null };
  const definitions = {
    "internal/shared": () => {
      const ObjectFreeze = Object.freeze;

      class NotImplementedError extends Error {
        code;
        constructor(feature, issue, extra) {
          super(feature + " is not yet implemented in Bun." + (issue ? " Track the status & thumbs up the issue: https://github.com/oven-sh/bun/issues/" + issue : "") + (extra ? ". " + extra : ""));
          this.name = "NotImplementedError";
          this.code = "ERR_NOT_IMPLEMENTED";
          hideFromStack(NotImplementedError);
        }
        get ["constructor"]() {
          return Error;
        }
      }
      function throwNotImplemented(feature, issue, extra) {
        hideFromStack(throwNotImplemented);
        throw new NotImplementedError(feature, issue, extra);
      }
      function hideFromStack(...fns) {
        for (const fn of fns) {
          Object.defineProperty(fn, "name", {
            value: "::bunternal::"
          });
        }
      }
      let warned;
      function warnNotImplementedOnce(feature, issue) {
        if (!warned) {
          warned = new Set;
        }
        if (warned.has(feature)) {
          return;
        }
        warned.add(feature);
        console.warn(new NotImplementedError(feature, issue));
      }
      let util;

      class ExceptionWithHostPort extends Error {
        errno;
        syscall;
        port;
        address;
        constructor(err, syscall, address, port, additional) {
          util ??= require("node:util");
          const code = util.getSystemErrorName(err);
          let details = "";
          if (port && port > 0) {
            details = ` ${address}:${port}`;
          } else if (address) {
            details = ` ${address}`;
          }
          if (additional) {
            details += ` - Local (${additional})`;
          }
          super(`${syscall} ${code}${details}`);
          this.errno = err;
          this.code = code;
          this.syscall = syscall;
          this.address = address;
          if (port) {
            this.port = port;
          }
        }
        get ["constructor"]() {
          return Error;
        }
      }

      class NodeAggregateError extends AggregateError {
        constructor(errors, message) {
          super(new (require("internal/primordials")).SafeArrayIterator(errors), message);
          this.code = errors[0]?.code;
        }
        get ["constructor"]() {
          return AggregateError;
        }
      }

      class ConnResetException extends Error {
        constructor(msg) {
          super(msg);
          this.code = "ECONNRESET";
        }
        get ["constructor"]() {
          return Error;
        }
      }

      class ErrnoException extends Error {
        errno;
        syscall;
        constructor(err, syscall, original) {
          util ??= require("node:util");
          const code = util.getSystemErrorName(err);
          const message = original ? `${syscall} ${code} ${original}` : `${syscall} ${code}`;
          super(message);
          this.errno = err;
          this.code = code;
          this.syscall = syscall;
        }
        get ["constructor"]() {
          return Error;
        }
      }
      function once(callback, { preserveReturnValue = false } = kEmptyObject) {
        let called = false;
        let returnValue;
        return function(...args) {
          if (called)
            return returnValue;
          called = true;
          const fn = callback;
          callback = undefined;
          const result = fn.apply(this, args);
          returnValue = preserveReturnValue ? result : undefined;
          return result;
        };
      }
      const kEmptyObject = ObjectFreeze(Object.create(null));
      const kInternalSendOptions = Object.create(null);
      ObjectFreeze(kInternalSendOptions);
      const reportUncaughtException = error => queueMicrotask(() => { throw error; });
      function guardCallback(callback) {
        return function guarded(a, b, c) {
          try {
            switch (arguments.length) {
              case 0:
                return callback();
              case 1:
                return callback(a);
              case 2:
                return callback(a, b);
              case 3:
                return callback(a, b, c);
              default:
                return callback.apply(undefined, arguments);
            }
          } catch (e) {
            reportUncaughtException(e);
          }
        };
      }
      const nodeModulesRE = /[\\/]node_modules[\\/]/;
      const ErrorCaptureStackTrace = Error.captureStackTrace;
      function returnStackFrames(_err, frames) {
        return frames;
      }
      function isInsideNodeModules(frameLimit) {
        let prevLimit, prevPrepare;
        let frames;
        try {
          prevLimit = Error.stackTraceLimit;
          prevPrepare = Error.prepareStackTrace;
          Error.stackTraceLimit = frameLimit;
          Error.prepareStackTrace = returnStackFrames;
          const target = {};
          ErrorCaptureStackTrace(target, isInsideNodeModules);
          frames = target.stack;
        } catch {} finally {
          try {
            Error.stackTraceLimit = prevLimit;
            Error.prepareStackTrace = prevPrepare;
          } catch {}
        }
        if (!Array.isArray(frames))
          return false;
        try {
          for (const frame of frames) {
            const filename = frame.getFileName();
            if (!filename || filename.startsWith("node:") || filename.startsWith("internal:") || filename === "native") {
              continue;
            }
            return nodeModulesRE.test(filename);
          }
        } catch {}
        return false;
      }
      function resistStopPropagation(options) {
        options[host.internal.kResistStopPropagation] = true;
        return options;
      }
      function getLazy(initializer) {
        let value;
        let initialized = false;
        return function() {
          if (initialized)
            return value;
          value = initializer();
          initialized = true;
          return value;
        };
      }
      const observerCounts = new Map;
      const kObservers = new Set;
      const kNodeEntryTypes = new Set(["net", "dns", "http", "http2", "function", "quic"]);
      function hasObserver(type) {
        return (observerCounts.get(type) ?? 0) > 0;
      }
      function enqueueNodeEntry(entry) {
        for (const observer of kObservers) {
          observer.bufferEntry(entry);
        }
      }

      class PerformanceNodeEntry {
        name;
        entryType;
        startTime;
        duration;
        detail;
        constructor(name, entryType, startTime, duration, detail) {
          this.name = name;
          this.entryType = entryType;
          this.startTime = startTime;
          this.duration = duration;
          this.detail = detail;
        }
        toJSON() {
          return {
            name: this.name,
            entryType: this.entryType,
            startTime: this.startTime,
            duration: this.duration,
            detail: this.detail
          };
        }
      }
      function startPerf(target, key, context) {
        context.startTime = performance.now();
        target[key] = context;
      }
      function stopPerf(target, key, context) {
        const ctx = target[key];
        if (!ctx) {
          return;
        }
        target[key] = undefined;
        const startTime = ctx.startTime;
        const detail = ctx.detail !== undefined || context?.detail !== undefined ? { ...ctx.detail, ...context?.detail } : undefined;
        enqueueNodeEntry(new PerformanceNodeEntry(ctx.name, ctx.type, startTime, performance.now() - startTime, detail));
      }

      class NodeEntryObserver {
        callback;
        owner;
        types = new Set;
        buffer = [];
        scheduled = false;
        constructor(callback, owner) {
          this.callback = callback;
          this.owner = owner;
        }
        observe(types) {
          for (const type of this.types) {
            observerCounts.set(type, (observerCounts.get(type) ?? 1) - 1);
          }
          this.types = new Set(types);
          for (const type of this.types) {
            observerCounts.set(type, (observerCounts.get(type) ?? 0) + 1);
          }
          kObservers.add(this);
        }
        disconnect() {
          for (const type of this.types) {
            observerCounts.set(type, (observerCounts.get(type) ?? 1) - 1);
          }
          this.types.clear();
          this.buffer = [];
          kObservers.delete(this);
        }
        bufferEntry(entry) {
          if (!this.types.has(entry.entryType)) {
            return;
          }
          this.buffer.push(entry);
          if (!this.scheduled) {
            this.scheduled = true;
            setImmediate(() => {
              this.scheduled = false;
              const entries = this.buffer;
              if (entries.length === 0) {
                return;
              }
              this.buffer = [];
              this.callback.call(undefined, makeNodeEntryList(entries), this.owner);
            });
          }
        }
      }
      function makeNodeEntryList(entries) {
        const sorted = entries.slice().sort((a, b) => a.startTime - b.startTime);
        return {
          getEntries() {
            return sorted.slice();
          },
          getEntriesByType(type) {
            return sorted.filter((entry) => entry.entryType === type);
          },
          getEntriesByName(name, type) {
            return sorted.filter((entry) => entry.name === name && (type === undefined || entry.entryType === type));
          }
        };
      }
      const kInternalAssertionSuffix = `
      This is caused by either a bug in Node.js or incorrect usage of Node.js internals.
      ` + `Please open an issue with this stack trace at https://github.com/nodejs/node/issues
      `;
      return {
        kInternalAssertionSuffix,
        throwNotImplemented,
        hideFromStack,
        warnNotImplementedOnce,
        ExceptionWithHostPort,
        NodeAggregateError,
        ConnResetException,
        ErrnoException,
        once,
        getLazy,
        guardCallback,
        isInsideNodeModules,
        reportUncaughtException,
        resistStopPropagation,
        hasObserver,
        startPerf,
        stopPerf,
        enqueueNodeEntry,
        kNodeEntryTypes,
        NodeEntryObserver,
        PerformanceNodeEntry,
        kHandle: Symbol("kHandle"),
        kClusterOwner: Symbol("kClusterOwner"),
        kAutoDestroyed: Symbol("kAutoDestroyed"),
        kWeakHandler: Symbol("kWeak"),
        kCustomPromisifyArgsSymbol: Symbol("customPromisifyArgs"),
        kEmptyObject,
        kInternalSendOptions
      };
    },
    "internal/timers": () => {
      const { validateNumber } = require("internal/validators");
      const NumberIsFinite = Number.isFinite;
      const TIMEOUT_MAX = 2 ** 31 - 1;
      function getTimerDuration(msecs, name) {
        validateNumber(msecs, name);
        if (msecs < 0 || !NumberIsFinite(msecs)) {
          throw codes.OUT_OF_RANGE(name, "a non-negative finite number", msecs);
        }
        if (msecs > TIMEOUT_MAX) {
          process.emitWarning(`${msecs} does not fit into a 32-bit signed integer.` + `
      Timer duration was truncated to ${TIMEOUT_MAX}.`, "TimeoutOverflowWarning");
          return TIMEOUT_MAX;
        }
        return msecs;
      }
      return {
        kTimeout: Symbol.for("::buntimeout::"),
        getTimerDuration
      };
    },
    "internal/net/isIP": () => {
      const v4Seg = "(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])";
      const v4Str = `(?:${v4Seg}\\.){3}${v4Seg}`;
      var IPv4Reg;
      const v6Seg = "(?:[0-9a-fA-F]{1,4})";
      var IPv6Reg;
      function isIPv4(s) {
        return (IPv4Reg ??= new RegExp(`^${v4Str}$`)).test(s);
      }
      function isIPv6(s) {
        return (IPv6Reg ??= new RegExp("^(?:" + `(?:${v6Seg}:){7}(?:${v6Seg}|:)|` + `(?:${v6Seg}:){6}(?:${v4Str}|:${v6Seg}|:)|` + `(?:${v6Seg}:){5}(?::${v4Str}|(?::${v6Seg}){1,2}|:)|` + `(?:${v6Seg}:){4}(?:(?::${v6Seg}){0,1}:${v4Str}|(?::${v6Seg}){1,3}|:)|` + `(?:${v6Seg}:){3}(?:(?::${v6Seg}){0,2}:${v4Str}|(?::${v6Seg}){1,4}|:)|` + `(?:${v6Seg}:){2}(?:(?::${v6Seg}){0,3}:${v4Str}|(?::${v6Seg}){1,5}|:)|` + `(?:${v6Seg}:){1}(?:(?::${v6Seg}){0,4}:${v4Str}|(?::${v6Seg}){1,6}|:)|` + `(?::(?:(?::${v6Seg}){0,5}:${v4Str}|(?::${v6Seg}){1,7}|:))` + ")(?:%[0-9a-zA-Z-.:]{1,})?$")).test(s);
      }
      function isIP(s) {
        if (isIPv4(s))
          return 4;
        if (isIPv6(s))
          return 6;
        return 0;
      }
      return {
        isIPv4,
        isIPv6,
        isIP
      };
    },
    "internal/net/symbols": () => {
      return {
        kArmHandshakeTimeout: Symbol("kArmHandshakeTimeout"),
        kSecureConnectDone: Symbol("kSecureConnectDone"),
        kVerifyError: Symbol("kVerifyError")
      };
    },
    "internal/async_hooks": () => {
      let activeHooks = 0;
      function enabledHooksExist() {
        return activeHooks > 0;
      }
      function markHookEnabled() {
        activeHooks += 1;
      }
      function markHookDisabled() {
        if (activeHooks > 0)
          activeHooks -= 1;
      }
      const owner_symbol = Symbol("owner_symbol");
      return {
        enabledHooksExist,
        markHookEnabled,
        markHookDisabled,
        symbols: { owner_symbol }
      };
    },
    "internal/async_context_frame": () => {
      const AsyncContextFrame = {
        enabled: true,
        current() {
          return host.getAsyncContext();
        },
        exchange(frame) {
          const prev = host.getAsyncContext();
          host.setAsyncContext(frame);
          return prev;
        },
        run(frame, fn, thisArg, ...args) {
          const prev = host.getAsyncContext();
          if (frame === prev)
            return fn.apply(thisArg, args);
          host.setAsyncContext(frame);
          try {
            return fn.apply(thisArg, args);
          } finally {
            host.setAsyncContext(prev);
          }
        }
      };
      return AsyncContextFrame;
    },
    "internal/cluster/isPrimary": () => {
      return {
        isPrimary: !Object.prototype.hasOwnProperty.call(process.env, "NODE_UNIQUE_ID")
      };
    },
    "internal/freelist": () => {
      class FreeList {
        name;
        max;
        ctor;
        list;
        constructor(name, max, ctor) {
          this.name = name;
          this.ctor = ctor;
          this.max = max;
          this.list = [];
        }
        alloc() {
          return this.list.length > 0 ? this.list.pop() : new this.ctor(...arguments);
        }
        free(obj) {
          if (this.list.length < this.max) {
            this.list.push(obj);
            return true;
          }
          return false;
        }
      }
      return FreeList;
    },
    "internal/promisify": () => {
      const kCustomPromisifiedSymbol = Symbol.for("nodejs.util.promisify.custom");
      const { validateFunction } = require("internal/validators");
      const { kCustomPromisifyArgsSymbol } = require("internal/shared");
      function defineCustomPromisify(target, callback) {
        Object.defineProperty(target, kCustomPromisifiedSymbol, {
          value: callback,
          configurable: true
        });
        return callback;
      }
      function defineCustomPromisifyArgs(target, args) {
        Object.defineProperty(target, kCustomPromisifyArgsSymbol, {
          value: args,
          enumerable: false
        });
        return args;
      }
      var promisify = function promisify(original) {
        validateFunction(original, "original");
        const custom = original[kCustomPromisifiedSymbol];
        if (custom) {
          validateFunction(custom, "custom");
          return defineCustomPromisify(custom, custom);
        }
        const callbackArgs = original[kCustomPromisifyArgsSymbol];
        function fn(...originalArgs) {
          const { promise, resolve, reject } = Promise.withResolvers();
          try {
            const maybePromise = original.apply(this, [
              ...originalArgs,
              function(err, ...values) {
                if (err) {
                  return reject(err);
                }
                if (callbackArgs !== undefined) {
                  const result = {};
                  for (let i = 0;i < callbackArgs.length; i++) {
                    result[callbackArgs[i]] = values[i];
                  }
                  resolve(result);
                } else {
                  resolve(values[0]);
                }
              }
            ]);
            if ((value => value instanceof Promise)(maybePromise)) {
              process.emitWarning("Calling promisify on a function that returns a Promise is likely a mistake.", "DeprecationWarning", "DEP0174");
            }
          } catch (err) {
            reject(err);
          }
          return promise;
        }
        Object.setPrototypeOf(fn, Object.getPrototypeOf(original));
        defineCustomPromisify(fn, fn);
        return Object.defineProperties(fn, Object.getOwnPropertyDescriptors(original));
      };
      promisify.custom = kCustomPromisifiedSymbol;
      return {
        defineCustomPromisifyArgs,
        promisify
      };
    },
    "internal/url": () => {
      function urlToHttpOptions(url) {
        const options = {
          ...url,
          protocol: url.protocol,
          hostname: typeof url.hostname === "string" && url.hostname.startsWith("[") ? url.hostname.slice(1, -1) : url.hostname,
          hash: url.hash,
          search: url.search,
          pathname: url.pathname,
          path: `${url.pathname || ""}${url.search || ""}`,
          href: url.href
        };
        const port = url.port;
        if (port !== "") {
          options.port = Number(port);
        }
        const username = url.username;
        let password;
        if (username || (password = url.password)) {
          options.auth = `${decodeURIComponent(username)}:${decodeURIComponent(password ?? url.password)}`;
        }
        return options;
      }
      return {
        urlToHttpOptions
      };
    },
    "internal/util/deprecate": () => {
      const { validateString } = require("internal/validators");
      const codesWarned = new Set;
      function getDeprecationWarningEmitter(code, msg, deprecated, shouldEmitWarning = () => true) {
        let warned = false;
        return function() {
          if (!warned && shouldEmitWarning()) {
            warned = true;
            if (code !== undefined) {
              if (!codesWarned.has(code)) {
                process.emitWarning(msg, "DeprecationWarning", code, deprecated);
                codesWarned.add(code);
              }
            } else {
              process.emitWarning(msg, "DeprecationWarning", deprecated);
            }
          }
        };
      }
      function deprecate(fn, msg, code) {
        if (code !== undefined)
          validateString(code, "code");
        const emitDeprecationWarning = getDeprecationWarningEmitter(code, msg, deprecated);
        function deprecated(...args) {
          if (!process.noDeprecation) {
            emitDeprecationWarning();
          }
          if (new.target) {
            return Reflect.construct(fn, args, new.target);
          }
          return fn.apply(this, args);
        }
        Object.setPrototypeOf(deprecated, fn);
        const fnPrototype = fn.prototype;
        if (fnPrototype) {
          Object.defineProperty(deprecated, "prototype", {
            __proto__: null,
            value: fnPrototype,
            writable: true
          });
        }
        Object.defineProperty(deprecated, "length", {
          __proto__: null,
          ...Object.getOwnPropertyDescriptor(fn, "length")
        });
        return deprecated;
      }
      return {
        deprecate
      };
    },
    "internal/http": () => {
      const { isIPv4 } = require("internal/net/isIP");
      const { setServerCustomOptions, setServerAppFlags, drainMicrotasks } = { setServerCustomOptions: notImplemented("http.Server"), setServerAppFlags: notImplemented("http.Server"), drainMicrotasks: notImplemented("http.Server") };
      const abortedSymbol = Symbol("aborted");
      const headerStateSymbol = Symbol("headerState");
      const eofInProgress = Symbol("eofInProgress");
      const fakeSocketSymbol = Symbol("fakeSocket");
      const isTlsSymbol = Symbol("is_tls");
      const kHandle = Symbol("handle");
      const kRealListen = Symbol("kRealListen");
      const noBodySymbol = Symbol("noBody");
      const optionsSymbol = Symbol("options");
      const tlsSymbol = Symbol("tls");
      const typeSymbol = Symbol("type");
      const kAbortController = Symbol.for("kAbortController");
      const kInternalSocketData = Symbol.for("::bunternal::");
      const serverSymbol = Symbol.for("::bunternal::");
      const kPendingCallbacks = Symbol("pendingCallbacks");
      const kRequest = Symbol("request");
      const kCloseCallback = Symbol("closeCallback");
      var NodeHTTPResponseAbortEvent;
      ((NodeHTTPResponseAbortEvent) => {
        NodeHTTPResponseAbortEvent[NodeHTTPResponseAbortEvent["none"] = 0] = "none";
        NodeHTTPResponseAbortEvent[NodeHTTPResponseAbortEvent["abort"] = 1] = "abort";
        NodeHTTPResponseAbortEvent[NodeHTTPResponseAbortEvent["timeout"] = 2] = "timeout";
      })(NodeHTTPResponseAbortEvent ||= {});
      var NodeHTTPIncomingRequestType;
      ((NodeHTTPIncomingRequestType) => {
        NodeHTTPIncomingRequestType[NodeHTTPIncomingRequestType["FetchRequest"] = 0] = "FetchRequest";
        NodeHTTPIncomingRequestType[NodeHTTPIncomingRequestType["FetchResponse"] = 1] = "FetchResponse";
        NodeHTTPIncomingRequestType[NodeHTTPIncomingRequestType["NodeHTTPResponse"] = 2] = "NodeHTTPResponse";
      })(NodeHTTPIncomingRequestType ||= {});
      var NodeHTTPBodyReadState;
      ((NodeHTTPBodyReadState) => {
        NodeHTTPBodyReadState[NodeHTTPBodyReadState["none"] = 0] = "none";
        NodeHTTPBodyReadState[NodeHTTPBodyReadState["pending"] = 2] = "pending";
        NodeHTTPBodyReadState[NodeHTTPBodyReadState["done"] = 4] = "done";
        NodeHTTPBodyReadState[NodeHTTPBodyReadState["hasBufferedDataDuringPause"] = 8] = "hasBufferedDataDuringPause";
      })(NodeHTTPBodyReadState ||= {});
      var NodeHTTPResponseFlags;
      ((NodeHTTPResponseFlags) => {
        NodeHTTPResponseFlags[NodeHTTPResponseFlags["socket_closed"] = 1] = "socket_closed";
        NodeHTTPResponseFlags[NodeHTTPResponseFlags["request_has_completed"] = 2] = "request_has_completed";
        NodeHTTPResponseFlags[NodeHTTPResponseFlags["ended"] = 4] = "ended";
        NodeHTTPResponseFlags[NodeHTTPResponseFlags["upgraded"] = 8] = "upgraded";
        NodeHTTPResponseFlags[NodeHTTPResponseFlags["closed_or_completed"] = 3] = "closed_or_completed";
      })(NodeHTTPResponseFlags ||= {});
      var NodeHTTPHeaderState;
      ((NodeHTTPHeaderState) => {
        NodeHTTPHeaderState[NodeHTTPHeaderState["none"] = 0] = "none";
        NodeHTTPHeaderState[NodeHTTPHeaderState["assigned"] = 1] = "assigned";
        NodeHTTPHeaderState[NodeHTTPHeaderState["sent"] = 2] = "sent";
      })(NodeHTTPHeaderState ||= {});
      function emitErrorNextTickIfErrorListenerNT(self, err, cb) {
        process.nextTick(emitErrorNextTickIfErrorListener, self, err, cb);
      }
      function emitErrorNextTickIfErrorListener(self, err, cb) {
        if ((typeof cb === "function")) {
          if (self.listenerCount("error") == 0) {
            cb();
          } else {
            cb(err);
          }
        }
      }
      function callCloseCallback(self) {
        if (self[kCloseCallback]) {
          self[kCloseCallback]();
          self[kCloseCallback] = undefined;
        }
      }
      function emitCloseNT(self) {
        if (!self._closed) {
          self.destroyed = true;
          self._closed = true;
          callCloseCallback(self);
          self.emit("close");
        }
      }
      function emitEOFIncomingMessageOuter(self) {
        self.complete = true;
        if (self[kHandle] !== undefined && !self[noBodySymbol]) {
          let rawTrailers = self[kHandle].takeRequestTrailers(self.socket?.server?.insecureHTTPParser === true);
          if (rawTrailers !== undefined) {
            const maxHeadersCount = self.socket?.server?.maxHeadersCount;
            if (typeof maxHeadersCount === "number" && maxHeadersCount > 0 && rawTrailers.length > maxHeadersCount * 2) {
              rawTrailers.length = maxHeadersCount * 2;
            }
            self._addHeaderLines(rawTrailers, rawTrailers.length);
          }
        }
        self.push(null);
        const socket = self.socket;
        if (socket != null) {
          const parser = socket.parser;
          if (parser != null && parser.incoming === self) {
            process.nextTick(clearServerParserIncoming, parser, self);
          }
        }
      }
      function clearServerParserIncoming(parser, req) {
        if (parser.incoming === req)
          parser.incoming = null;
      }
      function emitEOFIncomingMessage(self) {
        self[eofInProgress] = true;
        process.nextTick(emitEOFIncomingMessageOuter, self);
      }
      function onDataIncomingMessage(chunk, isLast, aborted) {
        if (aborted === 1 /* abort */) {
          return;
        }
        const socket = this.socket;
        socket?._unrefTimer?.();
        if (chunk && !this._dumped) {
          if (!this.push(chunk)) {
            if (this.upgrade)
              this[kHandle]?.pause();
            else if (socket && !socket.writableEnded)
              socket.pause();
          }
        }
        if (isLast) {
          emitEOFIncomingMessage(this);
          if (!this.upgrade && socket && !socket._paused && socket.readable)
            socket.resume();
        }
      }
      function validateMsecs(numberlike, field) {
        if (typeof numberlike !== "number" || numberlike < 0) {
          throw codes.INVALID_ARG_TYPE(field, "number", numberlike);
        }
        return numberlike;
      }
      const METHODS = [
        "ACL",
        "BIND",
        "CHECKOUT",
        "CONNECT",
        "COPY",
        "DELETE",
        "GET",
        "HEAD",
        "LINK",
        "LOCK",
        "M-SEARCH",
        "MERGE",
        "MKACTIVITY",
        "MKCALENDAR",
        "MKCOL",
        "MOVE",
        "NOTIFY",
        "OPTIONS",
        "PATCH",
        "POST",
        "PROPFIND",
        "PROPPATCH",
        "PURGE",
        "PUT",
        "QUERY",
        "REBIND",
        "REPORT",
        "SEARCH",
        "SOURCE",
        "SUBSCRIBE",
        "TRACE",
        "UNBIND",
        "UNLINK",
        "UNLOCK",
        "UNSUBSCRIBE"
      ];
      const STATUS_CODES = {
        100: "Continue",
        101: "Switching Protocols",
        102: "Processing",
        103: "Early Hints",
        200: "OK",
        201: "Created",
        202: "Accepted",
        203: "Non-Authoritative Information",
        204: "No Content",
        205: "Reset Content",
        206: "Partial Content",
        207: "Multi-Status",
        208: "Already Reported",
        226: "IM Used",
        300: "Multiple Choices",
        301: "Moved Permanently",
        302: "Found",
        303: "See Other",
        304: "Not Modified",
        305: "Use Proxy",
        307: "Temporary Redirect",
        308: "Permanent Redirect",
        400: "Bad Request",
        401: "Unauthorized",
        402: "Payment Required",
        403: "Forbidden",
        404: "Not Found",
        405: "Method Not Allowed",
        406: "Not Acceptable",
        407: "Proxy Authentication Required",
        408: "Request Timeout",
        409: "Conflict",
        410: "Gone",
        411: "Length Required",
        412: "Precondition Failed",
        413: "Payload Too Large",
        414: "URI Too Long",
        415: "Unsupported Media Type",
        416: "Range Not Satisfiable",
        417: "Expectation Failed",
        418: "I'm a Teapot",
        421: "Misdirected Request",
        422: "Unprocessable Entity",
        423: "Locked",
        424: "Failed Dependency",
        425: "Too Early",
        426: "Upgrade Required",
        428: "Precondition Required",
        429: "Too Many Requests",
        431: "Request Header Fields Too Large",
        451: "Unavailable For Legal Reasons",
        500: "Internal Server Error",
        501: "Not Implemented",
        502: "Bad Gateway",
        503: "Service Unavailable",
        504: "Gateway Timeout",
        505: "HTTP Version Not Supported",
        506: "Variant Also Negotiates",
        507: "Insufficient Storage",
        508: "Loop Detected",
        509: "Bandwidth Limit Exceeded",
        510: "Not Extended",
        511: "Network Authentication Required"
      };
      function hasServerResponseFinished(self, chunk, callback) {
        const finished = self.finished;
        if (chunk) {
          const destroyed = self.destroyed;
          if (finished || destroyed) {
            let err;
            if (finished) {
              err = codes.STREAM_WRITE_AFTER_END();
            } else if (destroyed) {
              err = codes.STREAM_DESTROYED("Stream is destroyed");
            }
            if (!destroyed) {
              process.nextTick(emitErrorNt, self, err, callback);
            } else if ((typeof callback === "function")) {
              process.nextTick(callback, err);
            }
            return true;
          }
        } else if (finished) {
          if ((typeof callback === "function")) {
            if (!self.writableFinished) {
              self.on("finish", callback);
            } else {
              callback(codes.STREAM_ALREADY_FINISHED("end"));
            }
          }
          return true;
        }
        return false;
      }
      function emitErrorNt(msg, err, callback) {
        if ((typeof callback === "function")) {
          callback(err);
        }
        if ((typeof msg.emit === "function") && !msg.destroyed) {
          msg.emit("error", err);
        }
      }
      const setMaxHTTPHeaderSize = httpBinding.setMaxHTTPHeaderSize;
      const getMaxHTTPHeaderSize = httpBinding.getMaxHTTPHeaderSize;
      const kOutHeaders = Symbol("kOutHeaders");
      const kNeedDrain = Symbol("kNeedDrain");
      const kProxyConfig = Symbol("kProxyConfig");
      const kWaitForProxyTunnel = Symbol("kWaitForProxyTunnel");
      let utcCache;
      function utcDate() {
        if (!utcCache)
          cacheUTCDate();
        return utcCache;
      }
      function cacheUTCDate() {
        const d = new Date;
        utcCache = d.toUTCString();
        const timer = setTimeout(resetUTCCache, 1000 - d.getMilliseconds());
        if (typeof timer.unref === "function")
          timer.unref();
      }
      function resetUTCCache() {
        utcCache = undefined;
      }
      function ipToInt(ip) {
        const octets = ip.split(".");
        let result = 0;
        for (let i = 0;i < octets.length; i++)
          result = (result << 8) + Number.parseInt(octets[i]);
        return result >>> 0;
      }

      class ProxyConfig {
        href;
        protocol;
        auth;
        bypassList;
        proxyConnectionOptions;
        constructor(proxyUrl, keepAlive, noProxyList) {
          let parsedURL;
          try {
            parsedURL = new URL(proxyUrl);
          } catch {
            throw codes.PROXY_INVALID_CONFIG(`Invalid proxy URL: ${proxyUrl}`);
          }
          const { hostname, port, protocol, username, password } = parsedURL;
          this.href = proxyUrl;
          this.protocol = protocol;
          if (username || password) {
            const auth = `${decodeURIComponent(username)}:${decodeURIComponent(password)}`;
            this.auth = `Basic ${Buffer.from(auth).toString("base64")}`;
          }
          if (noProxyList) {
            this.bypassList = noProxyList.split(",").map((entry) => entry.trim().toLowerCase());
          } else {
            this.bypassList = [];
          }
          this.proxyConnectionOptions = {
            host: hostname[0] === "[" ? hostname.slice(1, -1) : hostname,
            port: port ? Number(port) : protocol === "https:" ? 443 : 80
          };
        }
        shouldUseProxy(hostname, port) {
          const bypassList = this.bypassList;
          if (this.bypassList.length === 0)
            return true;
          const host = hostname.toLowerCase();
          const hostWithPort = port ? `${host}:${port}` : host;
          for (let i = 0;i < bypassList.length; i++) {
            const entry = bypassList[i];
            if (entry === "*")
              return false;
            if (entry === host || entry === hostWithPort)
              return false;
            if (entry.startsWith(".")) {
              const suffix = entry.substring(1);
              if (host === suffix || host.endsWith(suffix) && host[host.length - suffix.length - 1] === ".")
                return false;
            }
            if (entry.startsWith("*.") && host.endsWith(entry.substring(1)))
              return false;
            if (entry.includes("-") && isIPv4(host)) {
              let { 0: startIP, 1: endIP } = entry.split("-");
              startIP = startIP.trim();
              endIP = endIP.trim();
              if (startIP && endIP && isIPv4(startIP) && isIPv4(endIP)) {
                const hostInt = ipToInt(host);
                const startInt = ipToInt(startIP);
                const endInt = ipToInt(endIP);
                if (hostInt >= startInt && hostInt <= endInt)
                  return false;
              }
            }
          }
          return true;
        }
      }
      function parseProxyUrl(env, protocol) {
        const proxyUrl = protocol === "https:" ? env.https_proxy || env.HTTPS_PROXY : env.http_proxy || env.HTTP_PROXY;
        if (!proxyUrl) {
          return null;
        }
        if (proxyUrl.includes("\r") || proxyUrl.includes(`
      `)) {
          throw codes.PROXY_INVALID_CONFIG(`Invalid proxy URL: ${proxyUrl}`);
        }
        return proxyUrl;
      }
      function parseProxyConfigFromEnv(env, protocol, keepAlive) {
        if (protocol !== "http:" && protocol !== "https:")
          return null;
        const proxyUrl = parseProxyUrl(env, protocol);
        if (proxyUrl === null)
          return null;
        if (!proxyUrl.startsWith("http://") && !proxyUrl.startsWith("https://"))
          return null;
        return new ProxyConfig(proxyUrl, keepAlive, env.no_proxy || env.NO_PROXY);
      }
      function checkShouldUseProxy(proxyConfig, reqOptions) {
        if (!proxyConfig)
          return false;
        if (reqOptions.socketPath)
          return false;
        return proxyConfig.shouldUseProxy(reqOptions.host || "localhost", reqOptions.port);
      }

      return {
        NodeHTTPResponseAbortEvent,
        NodeHTTPIncomingRequestType,
        NodeHTTPBodyReadState,
        NodeHTTPResponseFlags,
        NodeHTTPHeaderState,
        METHODS,
        STATUS_CODES,
        abortedSymbol,
        callCloseCallback,
        checkShouldUseProxy,
        drainMicrotasks,
        emitCloseNT,
        emitEOFIncomingMessage,
        emitErrorNextTickIfErrorListenerNT,
        eofInProgress,
        fakeSocketSymbol,
        getMaxHTTPHeaderSize,
        hasServerResponseFinished,
        headerStateSymbol,
        isTlsSymbol,
        kAbortController,
        kCloseCallback,
        kHandle,
        kInternalSocketData,
        kNeedDrain,
        kOutHeaders,
        kPendingCallbacks,
        kProxyConfig,
        kRealListen,
        kRequest,
        kWaitForProxyTunnel,
        noBodySymbol,
        onDataIncomingMessage,
        optionsSymbol,
        parseProxyConfigFromEnv,
        parseProxyUrl,
        serverSymbol,
        setMaxHTTPHeaderSize,
        setServerAppFlags,
        setServerCustomOptions,
        tlsSymbol,
        typeSymbol,
        utcDate,
        validateMsecs
      };
    },
    "internal/http/FakeSocket": () => {
      const { kInternalSocketData, serverSymbol } = require("internal/http");
      const { kAutoDestroyed } = require("internal/shared");
      const { Duplex } = require("internal/stream");
      var FakeSocket = class Socket extends Duplex {
        [kInternalSocketData];
        bytesRead = 0;
        bytesWritten = 0;
        connecting = false;
        timeout = 0;
        isServer = false;
        #address;
        _httpMessage;
        constructor(httpMessage) {
          super();
          this._httpMessage = httpMessage;
        }
        address() {
          var internalData;
          return this.#address ??= (internalData = this[kInternalSocketData])?.[0]?.[serverSymbol]?.requestIP(internalData[2]) ?? {};
        }
        get bufferSize() {
          return this.writableLength;
        }
        connect(_port, _host, _connectListener) {
          return this;
        }
        _onTimeout = function() {
          this.emit("timeout");
        };
        _destroy(_err, _callback) {
          const socketData = this[kInternalSocketData];
          if (!socketData)
            return;
          if (!socketData[1]["req"][kAutoDestroyed])
            socketData[1].end();
        }
        _final(_callback) {}
        get localAddress() {
          return this.address() ? "127.0.0.1" : undefined;
        }
        get localFamily() {
          return "IPv4";
        }
        get localPort() {
          return 80;
        }
        get pending() {
          return this.connecting;
        }
        _read(_size) {}
        get readyState() {
          if (this.connecting)
            return "opening";
          if (this.readable) {
            return this.writable ? "open" : "readOnly";
          } else {
            return this.writable ? "writeOnly" : "closed";
          }
        }
        ref() {
          return this;
        }
        get remoteAddress() {
          return this.address()?.address;
        }
        set remoteAddress(val) {
          this.address().address = val;
        }
        get remotePort() {
          return this.address()?.port;
        }
        set remotePort(val) {
          this.address().port = val;
        }
        get remoteFamily() {
          return this.address()?.family;
        }
        set remoteFamily(val) {
          this.address().family = val;
        }
        resetAndDestroy() {}
        setKeepAlive(_enable = false, _initialDelay = 0) {}
        setNoDelay(_noDelay = true) {
          return this;
        }
        setTimeout(timeout, callback) {
          const socketData = this[kInternalSocketData];
          if (!socketData)
            return;
          const http_res = socketData[1];
          http_res?.req?.setTimeout(timeout, callback);
          return this;
        }
        unref() {
          return this;
        }
        _write(_chunk, _encoding, _callback) {}
        destroy() {
          this._httpMessage?.destroy?.();
          return super.destroy();
        }
      };
      Object.defineProperty(FakeSocket, "name", { value: "Socket" });
      return {
        FakeSocket
      };
    },
    "internal/http1_server_fallback": () => {
      const { STATUS_CODES } = require("internal/http");
      const { SafeSet } = require("internal/primordials");
      const kHttp1Connections = Symbol("http1Connections");
      const kHttp1ActiveRequests = Symbol("http1ActiveRequests");
      function createHttp1FallbackResponseHandle(socket, shouldKeepAlive, keepAliveTimeout) {
        const { _checkInvalidHeaderChar: checkInvalidHeaderChar } = require("node:_http_common");
        let head = null;
        let headWritten = false;
        let chunked = false;
        let noBody = false;
        let closeDelimited = false;
        function writeHeadToSocket(contentLength) {
          if (headWritten)
            return;
          headWritten = true;
          const statusCode = head?.statusCode ?? 200;
          let statusMessage = head?.statusMessage;
          if (typeof statusMessage !== "string" || statusMessage === "") {
            statusMessage = STATUS_CODES[statusCode] || "unknown";
          }
          let out = `HTTP/1.1 ${statusCode} ${statusMessage}\r
      `;
          let hasContentLength = false;
          let hasTransferEncoding = false;
          let hasDate = false;
          let hasConnection = false;
          let hasKeepAlive = false;
          const headers = head?.headers;
          if (headers) {
            for (let i = 0, end = headers.length - 1;i < end; i += 2) {
              const name = headers[i];
              const value = headers[i + 1];
              if (name.length === 1 && name.charCodeAt(0) === 0) {
                if (value === "2")
                  noBody = true;
                else
                  closeDelimited = true;
                continue;
              }
              switch (name.toLowerCase()) {
                case "content-length":
                  hasContentLength = true;
                  break;
                case "transfer-encoding":
                  hasTransferEncoding = true;
                  if (String(value).toLowerCase().includes("chunked"))
                    chunked = true;
                  break;
                case "date":
                  hasDate = true;
                  break;
                case "connection":
                  hasConnection = true;
                  break;
                case "keep-alive":
                  hasKeepAlive = true;
                  break;
              }
              out += `${name}: ${value}\r
      `;
            }
          }
          const autoBits = head?.autoHeaderBits ?? 0;
          const chunkedFromAutoBits = (autoBits & 16) !== 0;
          let autoContentLength = null;
          let autoChunked = false;
          if (!hasContentLength && !hasTransferEncoding && !noBody && !closeDelimited) {
            if (chunkedFromAutoBits || contentLength === null) {
              chunked = true;
              autoChunked = !chunkedFromAutoBits;
            } else {
              autoContentLength = contentLength;
            }
          }
          if (head === null) {
            if (!hasDate) {
              out += `Date: ${new Date().toUTCString()}\r
      `;
            }
            if (!hasConnection && !closeDelimited) {
              if (shouldKeepAlive) {
                out += `Connection: keep-alive\r
      `;
                if (!hasKeepAlive) {
                  out += `Keep-Alive: timeout=${Math.floor((keepAliveTimeout || 5000) / 1000)}\r
      `;
                }
              } else {
                out += `Connection: close\r
      `;
              }
            }
          } else {
            if (!hasDate && (autoBits & 1) !== 0) {
              out += `Date: ${new Date().toUTCString()}\r
      `;
            }
            if (!hasConnection) {
              if ((autoBits & 2) !== 0) {
                out += `Connection: keep-alive\r
      `;
                if (!hasKeepAlive && (autoBits & 8) !== 0) {
                  out += `Keep-Alive: timeout=${head.keepAliveTimeoutSecs}\r
      `;
                }
              } else if ((autoBits & 4) !== 0) {
                out += `Connection: close\r
      `;
              }
            }
          }
          if (autoContentLength !== null) {
            out += `Content-Length: ${autoContentLength}\r
      `;
          } else if (autoChunked || chunkedFromAutoBits && chunked) {
            out += `Transfer-Encoding: chunked\r
      `;
          }
          out += `\r
      `;
          socket.write(out);
        }
        function toBuffer(chunk, encoding) {
          if (chunk == null)
            return null;
          if (typeof chunk === "string")
            return Buffer.from(chunk, encoding || "utf8");
          return chunk;
        }
        function writeBody(buf) {
          const length = buf ? buf.byteLength ?? buf.length : 0;
          if (length) {
            if (chunked) {
              socket.write(length.toString(16) + `\r
      `);
              socket.write(buf);
              socket.write(`\r
      `);
            } else {
              socket.write(buf);
            }
          }
          return length;
        }
        const handle = {
          flags: 0,
          ended: false,
          finished: false,
          aborted: false,
          bufferedAmount: 0,
          shouldKeepAlive,
          onfinished: null,
          cork(callback) {
            return callback();
          },
          writeContinue() {
            socket.write(`HTTP/1.1 100 Continue\r
      \r
      `);
          },
          writeInformational(chunk, encoding) {
            socket.write(chunk, encoding);
          },
          writeHead(statusCode, statusMessage, headers, autoHeaderBits, keepAliveTimeoutSecs) {
            const originalStatusCode = statusCode;
            statusCode |= 0;
            if (statusCode < 100 || statusCode > 999) {
              throw codes.HTTP_INVALID_STATUS_CODE(`${originalStatusCode}`);
            }
            if (typeof statusMessage === "string" && checkInvalidHeaderChar(statusMessage)) {
              throw codes.INVALID_CHAR("statusMessage");
            }
            head = { statusCode, statusMessage, headers, autoHeaderBits, keepAliveTimeoutSecs };
          },
          flushHeaders() {
            writeHeadToSocket(null);
          },
          writeHeadAndEnd(statusCode, statusMessage, headers, chunk, encoding, strictContentLength, autoHeaderBits, keepAliveTimeoutSecs) {
            this.writeHead(statusCode, statusMessage, headers, autoHeaderBits, keepAliveTimeoutSecs);
            return this.end(chunk, encoding, undefined, strictContentLength);
          },
          write(chunk, encoding, _callback, _strictContentLength) {
            const buf = toBuffer(chunk, encoding);
            writeHeadToSocket(null);
            return writeBody(buf);
          },
          end(chunk, encoding, _callback, _strictContentLength) {
            if (this.ended)
              return 0;
            const buf = toBuffer(chunk, encoding);
            const length = buf ? buf.byteLength ?? buf.length : 0;
            writeHeadToSocket(length);
            writeBody(buf);
            if (chunked && !noBody)
              socket.write(`0\r
      \r
      `);
            this.ended = true;
            this.finished = true;
            const onfinished = this.onfinished;
            if (onfinished) {
              this.onfinished = null;
              onfinished();
            }
            if (closeDelimited && !socket.destroyed) {
              socket.end();
            }
            return length;
          },
          abort() {
            this.aborted = true;
            if (!socket.destroyed)
              socket.destroy();
          }
        };
        return handle;
      }
      function connectionListenerHTTP1(server, socket, options) {
        const http = require("node:http");
        const { HTTPParser, prepareError, calculateLenientFlags, continueExpression } = require("node:_http_common");
        const { kHandle: kHttp1ResponseHandle } = require("internal/http");
        const { allMethods } = require("native/http_parser");
        const http1Options = options.http1Options || {};
        const IncomingMessageClass = http1Options.IncomingMessage || http.IncomingMessage;
        const ServerResponseClass = http1Options.ServerResponse || http.ServerResponse;
        const keepAliveTimeout = typeof server.keepAliveTimeout === "number" ? server.keepAliveTimeout : 5000;
        socket.server = server;
        const connections = server[kHttp1Connections] ??= new SafeSet;
        connections.add(socket);
        socket[kHttp1ActiveRequests] = 0;
        const kOnHeadersComplete = HTTPParser.kOnHeadersComplete | 0;
        const kOnBody = HTTPParser.kOnBody | 0;
        const kOnMessageComplete = HTTPParser.kOnMessageComplete | 0;
        const lenientFlags = calculateLenientFlags(server.httpValidation, server.insecureHTTPParser);
        const parser = new HTTPParser;
        parser.initialize(HTTPParser.REQUEST, {}, server.maxHeaderSize || 0, lenientFlags);
        parser.socket = socket;
        socket.parser = parser;
        const { maxHeadersCount } = server;
        if (typeof maxHeadersCount === "number") {
          parser.maxHeaderPairs = maxHeadersCount << 1;
        }
        let req = null;
        let pendingUpgrade = null;
        parser[kOnHeadersComplete] = function onHttp1HeadersComplete(versionMajor, versionMinor, rawHeaders, methodNum, url, _statusCode, _statusMessage, upgrade, shouldKeepAlive) {
          socket[kHttp1ActiveRequests]++;
          req = new IncomingMessageClass(socket);
          req.socket = socket;
          req.httpVersionMajor = versionMajor;
          req.httpVersionMinor = versionMinor;
          req.httpVersion = `${versionMajor}.${versionMinor}`;
          req.url = url;
          req.method = typeof methodNum === "number" ? allMethods[methodNum] : methodNum;
          req.upgrade = upgrade;
          req._addHeaderLines(rawHeaders, rawHeaders.length);
          if (upgrade) {
            req.upgrade = req.method === "CONNECT" || (typeof server.shouldUpgradeCallback === "function" ? !!server.shouldUpgradeCallback(req) : server.listenerCount("upgrade") > 0);
            if (req.upgrade) {
              pendingUpgrade = req;
              return 2;
            }
          }
          req._read = function(_size) {
            if (socket.readable)
              socket.resume();
          };
          const res = new ServerResponseClass(req);
          res._keepAliveTimeout = keepAliveTimeout;
          res._maxRequestsPerSocket = server.maxRequestsPerSocket;
          const handle = createHttp1FallbackResponseHandle(socket, shouldKeepAlive, keepAliveTimeout);
          handle.onfinished = function() {
            socket[kHttp1ActiveRequests] = Math.max(0, (socket[kHttp1ActiveRequests] || 1) - 1);
            if (!shouldKeepAlive && !socket.destroyed) {
              socket.end();
            }
          };
          res[kHttp1ResponseHandle] = handle;
          res.assignSocket(socket);
          res.on("finish", function onFallbackResponseFinish() {
            this.detachSocket(socket);
          });
          const expect = req.headers.expect;
          if (expect !== undefined && versionMajor === 1 && versionMinor === 1) {
            if (continueExpression.test(String(expect))) {
              if (server.listenerCount("checkContinue") > 0) {
                server.emit("checkContinue", req, res);
              } else {
                res.writeContinue();
                server.emit("request", req, res);
              }
            } else if (server.listenerCount("checkExpectation") > 0) {
              server.emit("checkExpectation", req, res);
            } else {
              res.writeHead(417);
              res.end();
            }
            return 0;
          }
          server.emit("request", req, res);
          return 0;
        };
        parser[kOnBody] = function onHttp1Body(chunk) {
          if (req && !req._dumped)
            req.push(chunk);
        };
        parser[kOnMessageComplete] = function onHttp1MessageComplete() {
          if (req) {
            req.complete = true;
            req.push(null);
          }
        };
        function onHttp1SocketError(err, rawPacket) {
          prepareError(err, parser, rawPacket);
          if (!server.emit("clientError", err, socket)) {
            if (socket.writable && !socket.destroyed) {
              const code = err?.code;
              socket.write(code === "HPE_HEADER_OVERFLOW" ? `HTTP/1.1 431 Request Header Fields Too Large\r
      Connection: close\r
      \r
      ` : code === "HPE_CHUNK_EXTENSIONS_OVERFLOW" ? `HTTP/1.1 413 Payload Too Large\r
      Connection: close\r
      \r
      ` : `HTTP/1.1 400 Bad Request\r
      Connection: close\r
      \r
      `, "latin1");
            }
            socket.destroy(err);
          }
        }
        function onHttp1SocketData(data) {
          const ret = parser.execute(data);
          if (ret instanceof Error) {
            onHttp1SocketError(ret, data);
            return;
          }
          if (pendingUpgrade) {
            const upgradeReq = pendingUpgrade;
            pendingUpgrade = null;
            socket.removeListener("data", onHttp1SocketData);
            socket.removeListener("error", onHttp1SocketErrorListener);
            socket.removeListener("end", onHttp1SocketEnd);
            connections.delete(socket);
            try {
              parser.close();
            } catch {}
            socket.parser = null;
            const eventName = upgradeReq.method === "CONNECT" ? "connect" : "upgrade";
            const bodyHead = typeof ret === "number" ? data.slice(ret) : Buffer.alloc(0);
            if (server.listenerCount(eventName) > 0) {
              socket.readableFlowing = null;
              server.emit(eventName, upgradeReq, socket, bodyHead);
            } else {
              socket.destroy();
            }
          }
        }
        function onHttp1SocketErrorListener(err) {
          onHttp1SocketError(err, undefined);
        }
        function onHttp1SocketEnd() {
          const ret = parser.finish();
          if (ret instanceof Error) {
            onHttp1SocketError(ret, undefined);
            return;
          }
          if (!server.httpAllowHalfOpen) {
            if (req && !req.complete)
              req.destroy();
            if (socket.writable)
              socket.end();
            return;
          }
          const httpMessage = socket._httpMessage;
          if (httpMessage) {
            httpMessage._last = true;
          } else if (socket.writable) {
            socket.end();
          }
        }
        socket.on("data", onHttp1SocketData);
        socket.on("error", onHttp1SocketErrorListener);
        socket.once("end", onHttp1SocketEnd);
        socket.once("close", () => {
          connections.delete(socket);
          try {
            parser.close();
          } catch {}
        });
      }
      function closeIdleHttp1Connections(server) {
        const connections = server[kHttp1Connections];
        if (!connections)
          return;
        for (const socket of connections) {
          if (!socket[kHttp1ActiveRequests] && !socket.destroyed) {
            socket.destroy();
          }
        }
      }
      function closeAllHttp1Connections(server) {
        const connections = server[kHttp1Connections];
        if (!connections)
          return;
        for (const socket of connections) {
          if (!socket.destroyed)
            socket.destroy();
        }
      }
      return {
        connectionListenerHTTP1,
        closeIdleHttp1Connections,
        closeAllHttp1Connections,
        kHttp1Connections
      };
    },
    "internal/tls": () => {
      const { isTypedArray, isArrayBuffer } = require("node:util/types");
      function isPemObject(obj) {
        return isObject(obj) && "pem" in obj;
      }
      function isPemArray(obj) {
        return Array.isArray(obj) && obj.every(isPemObject);
      }
      function isValidTLSItem(obj) {
        if (typeof obj === "string" || isTypedArray(obj) || isArrayBuffer(obj) || isBlob(obj) || isPemArray(obj)) {
          return true;
        }
        return false;
      }
      function findInvalidTLSItem(obj) {
        if (Array.isArray(obj)) {
          for (var i = 0, length = obj.length;i < length; i++) {
            const item = obj[i];
            if (!isValidTLSItem(item))
              return item;
          }
        }
        return obj;
      }
      function throwOnInvalidTLSArray(name, value) {
        if (!isValidTLSArray(value)) {
          throw codes.INVALID_ARG_TYPE(name, VALID_TLS_ERROR_MESSAGE_TYPES, findInvalidTLSItem(value));
        }
      }
      function isValidTLSArray(obj) {
        if (isValidTLSItem(obj))
          return true;
        if (Array.isArray(obj)) {
          for (var i = 0, length = obj.length;i < length; i++) {
            const item = obj[i];
            if (!isValidTLSItem(item))
              return false;
          }
          return true;
        }
        return false;
      }
      const VALID_TLS_ERROR_MESSAGE_TYPES = "string or an instance of Buffer, TypedArray, or DataView";
      const TLS1_VERSION = 769;
      const TLS1_1_VERSION = 770;
      const TLS1_2_VERSION = 771;
      const TLS1_3_VERSION = 772;
      function tlsStringToProtocolVersion(v) {
        switch (v) {
          case "TLSv1":
            return TLS1_VERSION;
          case "TLSv1.1":
            return TLS1_1_VERSION;
          case "TLSv1.2":
            return TLS1_2_VERSION;
          case "TLSv1.3":
            return TLS1_3_VERSION;
          default:
            return 0;
        }
      }
      let _SECURE_PROTOCOL_METHODS;
      function validateSecureProtocol(secureProtocol) {
        if (secureProtocol === undefined || secureProtocol === null)
          return;
        if (typeof secureProtocol !== "string") {
          throw codes.INVALID_ARG_TYPE("options.secureProtocol", "string", secureProtocol);
        }
        let message;
        if (secureProtocol.startsWith("SSLv2_"))
          message = "SSLv2 methods disabled";
        else if (secureProtocol.startsWith("SSLv3_"))
          message = "SSLv3 methods disabled";
        else {
          _SECURE_PROTOCOL_METHODS ??= new Set([
            "TLS_method",
            "TLS_client_method",
            "TLS_server_method",
            "SSLv23_method",
            "SSLv23_client_method",
            "SSLv23_server_method",
            "TLSv1_method",
            "TLSv1_client_method",
            "TLSv1_server_method",
            "TLSv1_1_method",
            "TLSv1_1_client_method",
            "TLSv1_1_server_method",
            "TLSv1_2_method",
            "TLSv1_2_client_method",
            "TLSv1_2_server_method"
          ]);
          if (!_SECURE_PROTOCOL_METHODS.has(secureProtocol))
            message = `Unknown method: ${secureProtocol}`;
        }
        if (message !== undefined)
          throw codes.TLS_INVALID_PROTOCOL_METHOD(message);
      }
      function secureProtocolToVersionRange(secureProtocol) {
        if (typeof secureProtocol !== "string")
          return null;
        if (secureProtocol === "TLSv1_method" || secureProtocol === "TLSv1_client_method" || secureProtocol === "TLSv1_server_method")
          return [TLS1_VERSION, TLS1_VERSION];
        if (secureProtocol === "TLSv1_1_method" || secureProtocol === "TLSv1_1_client_method" || secureProtocol === "TLSv1_1_server_method")
          return [TLS1_1_VERSION, TLS1_1_VERSION];
        if (secureProtocol === "TLSv1_2_method" || secureProtocol === "TLSv1_2_client_method" || secureProtocol === "TLSv1_2_server_method")
          return [TLS1_2_VERSION, TLS1_2_VERSION];
        return null;
      }
      let NativeSecureContext;
      function processPfxOptions(options) {
        if (options == null || options.pfx == null)
          return options;
        NativeSecureContext ??= require("native/secure-context");
        const out = { ...options };
        const keys = out.key == null ? [] : Array.isArray(out.key) ? [...out.key] : [out.key];
        const certs = out.cert == null ? [] : Array.isArray(out.cert) ? [...out.cert] : [out.cert];
        const pfxCAs = [];
        const entries = Array.isArray(out.pfx) ? out.pfx : [out.pfx];
        for (const entry of entries) {
          let buf = entry;
          let passphrase = out.passphrase;
          if (entry != null && typeof entry === "object" && !Buffer.isBuffer(entry) && !(ArrayBuffer.isView(entry) && !(entry instanceof DataView))) {
            const entryBuf = entry.buf;
            if (entryBuf !== undefined) {
              buf = entryBuf;
              passphrase = entry.passphrase || passphrase;
            }
          }
          const parsed = NativeSecureContext.parsePkcs12(buf, passphrase);
          keys.push(parsed.key);
          certs.push(parsed.cert);
          const parsedCA = parsed.ca;
          if (parsedCA)
            pfxCAs.push(parsedCA);
        }
        out.key = keys.length === 1 ? keys[0] : keys;
        out.cert = certs.length === 1 ? certs[0] : certs;
        if (pfxCAs.length)
          out._pfxExtraCACerts = pfxCAs;
        out.pfx = undefined;
        return out;
      }

      return {
        processPfxOptions,
        secureProtocolToVersionRange,
        throwOnInvalidTLSArray,
        tlsStringToProtocolVersion,
        validateSecureProtocol
      };
    },
    "native/secure-context": define_native_secure_context,
    "native/http_parser": define_native_http_parser,
    "native/native-readable": define_native_native_readable,
    "internal/trace_events": define_internal_trace_events,
  };
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/primordials") return host.internal;
    if (name === "internal/util/inspect") return host.requireBuiltin("node:util");
    if (name.startsWith("internal/stream") || name === "internal/abort_listener") return host.requireInternal(name);
    if (name in definitions) {
      if (!(name in cache)) cache[name] = definitions[name]();
      return cache[name];
    }
    return host.requireBuiltin(name);
  };
  return require;
})
