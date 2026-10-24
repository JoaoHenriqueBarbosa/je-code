// GERADO por `scripts/gen-node-net.js` a partir de `src/js/node/net.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { validators, toClass, codes, makeAbortError } = host.internal;
  const debug = () => {};
  const assert = () => {};
  const isObject = value => value !== null && (typeof value === "object" || typeof value === "function");
  // O que ainda não existe (TLS, `BlockList`...): lança no ponto de uso, nunca finge.
  const notImplemented = what => () => host.internal.throwNotImplemented(what);
  const socketInternals = host.socketInternals();
  const uvConstants = { UV_ECANCELED: -125, UV_ENOBUFS: -105, UV_ETIMEDOUT: -110 };
  // O estado de `autoSelectFamily` do `node_net_binding.rs` do bun.
  let autoSelectFamilyDefault = true;
  let autoSelectFamilyAttemptTimeoutDefault = 500;
  const binding = {
    getDefaultAutoSelectFamily: () => autoSelectFamilyDefault,
    setDefaultAutoSelectFamily: value => {
      if (typeof value !== "boolean") throw codes.INVALID_ARG_TYPE("autoSelectFamilyDefault", "boolean", value);
      autoSelectFamilyDefault = value;
      return value;
    },
    getDefaultAutoSelectFamilyAttemptTimeout: () => autoSelectFamilyAttemptTimeoutDefault,
    setDefaultAutoSelectFamilyAttemptTimeout: value => {
      validators.validateInt32(value, "value", 1);
      autoSelectFamilyAttemptTimeoutDefault = value < 10 ? 10 : value;
      return autoSelectFamilyAttemptTimeoutDefault;
    },
  };
  const require = name => {
    if (name === "internal/validators") return validators;
    if (name === "internal/streams/duplex" || name === "internal/streams/state" || name === "internal/abort_listener") return host.requireInternal(name);
    // Nenhuma categoria de trace está ligada: o `isCategoryGroupEnabled` responde que não e nada é emitido.
    if (name === "internal/trace_events") return { isCategoryGroupEnabled: () => false, emitEvent() {}, emitEventWithArgs() {} };
    if (name.startsWith("internal/")) return host.internalModule(name);
    return host.requireBuiltin(name);
  };
  return (() => {
    const Duplex = require("internal/streams/duplex");
    const { isUint8Array } = require("node:util/types");
    const { getDefaultHighWaterMark } = require("internal/streams/state");
    const EventEmitter = require("node:events");
    let dns;
    const normalizedArgsSymbol = Symbol("normalizedArgs");
    const {
      ExceptionWithHostPort,
      ConnResetException,
      NodeAggregateError,
      ErrnoException,
      hasObserver,
      startPerf,
      stopPerf
    } = require("internal/shared");
    const { kTimeout, getTimerDuration } = require("internal/timers");
    const { validateFunction, validateNumber, validateAbortSignal, validatePort, validateBoolean, validateInt32, validateString } = require("internal/validators");
    const { isIPv4, isIPv6, isIP } = require("internal/net/isIP");
    const { kArmHandshakeTimeout, kSecureConnectDone, kVerifyError } = require("internal/net/symbols");
    const ArrayPrototypeIncludes = Array.prototype.includes;
    const ArrayPrototypeJoin = Array.prototype.join;
    const ArrayPrototypePush = Array.prototype.push;
    const MathMax = Math.max;
    const MathMin = Math.min;
    let uvBinding;
    function uv() {
      return uvBinding ??= uvConstants;
    }
    const isWindows = process.platform === "win32";
    const getDefaultAutoSelectFamily = binding.getDefaultAutoSelectFamily;
    const setDefaultAutoSelectFamily = binding.setDefaultAutoSelectFamily;
    const getDefaultAutoSelectFamilyAttemptTimeout = binding.getDefaultAutoSelectFamilyAttemptTimeout;
    const setDefaultAutoSelectFamilyAttemptTimeout = binding.setDefaultAutoSelectFamilyAttemptTimeout;
    let tlsKeylogPath;
    let tlsKeylogWarned = false;
    function appendTlsKeylog(line) {
      if (!tlsKeylogWarned) {
        tlsKeylogWarned = true;
        process.emitWarning("Using --tls-keylog makes TLS connections insecure by writing secret key material to file " + tlsKeylogPath);
      }
      try {
        require("node:fs").appendFileSync(tlsKeylogPath, line, { mode: 384 });
      } catch {}
    }
    {
      const execArgv = process.execArgv;
      for (let i = 0;i < execArgv.length; i++) {
        const arg = execArgv[i];
        if (arg === "--no-network-family-autoselection" || arg === "--no-enable-network-family-autoselection") {
          setDefaultAutoSelectFamily(false);
        } else if (arg === "--network-family-autoselection" || arg === "--enable-network-family-autoselection") {
          setDefaultAutoSelectFamily(true);
        } else if (arg.startsWith("--network-family-autoselection-attempt-timeout=")) {
          const value = Number(arg.slice(arg.indexOf("=") + 1));
          if (Number.isFinite(value) && value >= 1)
            setDefaultAutoSelectFamilyAttemptTimeout(value);
        } else if (arg === "--network-family-autoselection-attempt-timeout" && i + 1 < execArgv.length) {
          const value = Number(execArgv[i + 1]);
          if (Number.isFinite(value) && value >= 1)
            setDefaultAutoSelectFamilyAttemptTimeout(value);
        } else if (arg.startsWith("--tls-keylog=")) {
          tlsKeylogPath = arg.slice("--tls-keylog=".length);
        } else if (arg === "--tls-keylog" && i + 1 < execArgv.length) {
          tlsKeylogPath = execArgv[i + 1];
        }
      }
    }
    let BlockList, SocketAddress;
    function lazyBlockList() {
      return BlockList ??= notImplemented("net.BlockList");
    }
    const newDetachedSocket = socketInternals.newDetachedSocket;
    const doConnect = socketInternals.doConnect;
    const addServerName = notImplemented("tls server name");
    const upgradeDuplexToTLS = notImplemented("tls upgrade");
    const upgradeTLSDeferred = notImplemented("tls upgrade");
    const isNamedPipeSocket = () => false;
    const getBufferedAmount = socketInternals.getBufferedAmount;
    const bunTlsSymbol = Symbol.for("::buntls::");
    const bunSocketServerOptions = Symbol.for("::bunnetserveroptions::");
    const { owner_symbol } = require("internal/async_hooks").symbols;
    const kServerSocket = Symbol("kServerSocket");
    const kBytesWritten = Symbol("kBytesWritten");
    const bunTLSConnectOptions = Symbol.for("::buntlsconnectoptions::");
    const kNativeSecureContextCtor = Symbol.for("::buntlsnativesecurecontextctor::");
    const kReinitializeHandle = Symbol("kReinitializeHandle");
    const kRealListen = Symbol("kRealListen");
    const kSetNoDelay = Symbol("kSetNoDelay");
    const kSetTOS = Symbol("kSetTOS");
    const kSetKeepAlive = Symbol("kSetKeepAlive");
    const kSyncWriteFd = Symbol("kSyncWriteFd");
    const kSetKeepAliveInitialDelay = Symbol("kSetKeepAliveInitialDelay");
    const kConnectOptions = Symbol("connect-options");
    const kAttach = Symbol("kAttach");
    const kCloseRawConnection = Symbol("kCloseRawConnection");
    const kupgraded = Symbol("kupgraded");
    const kAdoptedTLSRaw = Symbol("kAdoptedTLSRaw");
    const ksocket = Symbol("ksocket");
    const khandlers = Symbol("khandlers");
    const kclosed = Symbol("closed");
    const kended = Symbol("ended");
    const kpendingSession = Symbol("pendingSession");
    const kSNIError = Symbol("kSNIError");
    const kALPNError = Symbol("kALPNError");
    const kPerfHooksNetConnectContext = Symbol("kPerfHooksNetConnectContext");
    const khandshakeTimer = Symbol("khandshakeTimer");
    const kerrorEmitted = Symbol("kerrorEmitted");
    const kUserUnrefed = Symbol("kUserUnrefed");
    const kPausedUnref = Symbol("kPausedUnref");
    const kOnreadDeliver = Symbol("kOnreadDeliver");
    function noop() {}
    function onUpgradeAttachedWrite(chunk, encoding, callback, onClose) {
      this.off("close", onClose);
      this._write(chunk, encoding, callback);
    }
    function onUpgradeWriteClose(callback) {
      if (this[kwriteCallback] !== callback)
        return;
      this[kwriteCallback] = null;
      callback(codes.SOCKET_CLOSED());
    }
    const kUpgradeAttached = Symbol("kUpgradeAttached");
    const kOnreadTail = Symbol("kOnreadTail");
    const kOnreadDraining = Symbol("kOnreadDraining");
    const kOnreadBuffer = Symbol("kOnreadBuffer");
    const kOnreadPendingEnd = Symbol("kOnreadPendingEnd");
    const kOnreadReadRequested = Symbol("kOnreadReadRequested");
    const kOnreadEmptyTail = Buffer.alloc(0);
    const kwriteCallback = Symbol("writeCallback");
    const kSocketClass = Symbol("kSocketClass");
    function failWrite(self, negErrno, callback) {
      let er = new ErrnoException(negErrno, "write");
      if (typeof er.code !== "string" || !/^E[A-Z0-9]+$/.test(er.code)) {
        er = new ConnResetException("write ECONNRESET");
        er.errno = negErrno;
        er.syscall = "write";
      }
      self._pendingData = null;
      self[kwriteCallback] = null;
      if (callback) {
        callback(er);
        if (!self.destroyed && !self._hadError) {
          self._hadError = true;
          self.destroy(er);
        }
      } else if (!self.destroyed) {
        if (self.listenerCount("error") > 0) {
          self.once("error", () => {});
          self.destroy(er);
        } else {
          self.destroy();
        }
      }
    }
    function endNT(socket, callback, err) {
      socket.shutdown();
      callback(err);
    }
    function emitCloseNT(self, hasError) {
      self.emit("close", hasError);
    }
    function closeAdoptedTLSRawNT(handle, self, isException) {
      setImmediate(closeAdoptedTLSRawNowNT, handle, self, isException);
    }
    function closeAdoptedTLSRawNowNT(handle, self, isException) {
      handle.close(onSocketHandleClosed);
      setImmediate(emitCloseNT, self, isException);
    }
    function detachSocket(self) {
      if (!self)
        self = this;
      self._handle = null;
    }
    function destroyNT(self, err) {
      self.destroy(err);
    }
    let addAbortListener;
    function destroyWhenAborted(err) {
      if (!this.destroyed) {
        this.destroy(makeAbortError(undefined, { cause: err?.target?.reason }));
      }
    }
    function onSocketEnd() {
      if (!this.allowHalfOpen) {
        this.write = writeAfterFIN;
      }
    }
    function writeAfterFIN(chunk, encoding, cb) {
      if (!this.writableEnded) {
        return Duplex.prototype.write.call(this, chunk, encoding, cb);
      }
      if (typeof encoding === "function") {
        cb = encoding;
        encoding = null;
      }
      const err = new Error("This socket has been ended by the other party");
      err.code = "EPIPE";
      if (typeof cb === "function") {
        process.nextTick(cb, err);
      }
      this.destroy(err);
      return false;
    }
    function onClientHandshakeComplete(self, socket, verifyError) {
      self._securePending = false;
      self._secureEstablished = true;
      self[kVerifyError] = verifyError ?? null;
      self.alpnProtocol = socket.alpnProtocol;
      try {
        const { checkServerIdentity } = self[bunTLSConnectOptions];
        if (!verifyError && !self.isSessionReused() && typeof checkServerIdentity === "function") {
          const options = self[kConnectOptions];
          const hostname = self.servername || options?.host || options?.socket?._host || self._host || "localhost";
          const cert = self.getPeerCertificate(true);
          if (cert) {
            verifyError = checkServerIdentity(hostname, cert);
          }
        }
        let rejectUnauthorized;
        if (self._requestCert || (rejectUnauthorized = self._rejectUnauthorized)) {
          if (verifyError) {
            self.authorized = false;
            self.authorizationError = verifyError.code || verifyError.message;
            if (rejectUnauthorized ?? self._rejectUnauthorized) {
              self.destroy(verifyError);
              self.emit("secure", self);
              return;
            }
          } else {
            self.authorized = true;
          }
        } else {
          self.authorized = true;
        }
        self.secureConnecting = false;
        self.emit(kSecureConnectDone);
        self.emit("secureConnect", verifyError);
        const pendingSession = self[kpendingSession];
        if (pendingSession) {
          self[kpendingSession] = null;
          self.emit("session", pendingSession);
        }
        self.removeListener("end", onConnectEnd);
        self.emit("secure", self);
      } catch (err) {
        reportError(err);
      }
    }
    function onConnectEnd() {
      if (!this._hadError && this.secureConnecting) {
        const options = this[kConnectOptions];
        this._hadError = true;
        const error = new ConnResetException("Client network socket disconnected before secure TLS connection was established");
        error.path = options.path;
        error.host = options.host;
        error.port = options.port;
        error.localAddress = options.localAddress;
        this.destroy(error);
      }
    }
    function tlsHandshakeError(verifyError) {
      const verifyErrorCode = verifyError ? verifyError.code : undefined;
      if (verifyErrorCode && verifyErrorCode !== "ECONNRESET") {
        const reason = verifyError.reason || verifyError.message || "TLS handshake failed";
        const err = new Error(reason);
        const match = /^error:[0-9a-f]+:SSL routines:([^:]*):(.+)$/.exec(reason);
        if (match) {
          err.library = "SSL routines";
          err.function = match[1];
          err.reason = match[2];
          err.code = `ERR_SSL_${match[2]}`;
        } else {
          err.code = verifyErrorCode;
        }
        return err;
      }
      return new ConnResetException("socket hang up");
    }
    const SocketHandlers = {
      close(socket, err) {
        const self = socket.data;
        if (!self || self[kclosed])
          return;
        self[kclosed] = true;
        detachSocket(self);
        SocketEmitEndNT(self, err);
        self.data = null;
      },
      data(socket, buffer) {
        const { data: self } = socket;
        if (!self)
          return;
        self._unrefTimer();
        self.bytesRead += buffer.length;
        if (!self.push(buffer)) {
          readStop(self, socket);
        }
      },
      drain(socket) {
        const self = socket.data;
        if (!self)
          return;
        const callback = self[kwriteCallback];
        self.connecting = false;
        if (callback) {
          const writeChunk = self._pendingData;
          const res = socketInternals.write(socket, writeChunk || "", self._pendingEncoding || "utf8");
          if (res < 0) {
            failWrite(self, res, callback);
          } else if (res) {
            self._pendingData = self[kwriteCallback] = null;
            unrefAfterDrain(self, socket);
            callback(null);
          } else {
            self._pendingData = null;
          }
          self[kBytesWritten] = socket.bytesWritten;
        }
      },
      end(socket) {
        const self = socket.data;
        if (!self)
          return;
        SocketEmitEndNT(self);
      },
      session(socket, session) {
        const self = socket.data;
        if (!self)
          return;
        if (self._secureEstablished) {
          self.emit("session", session);
        } else {
          self[kpendingSession] = session;
        }
      },
      keylog(socket, line) {
        const self = socket.data;
        if (!self)
          return;
        self.emit("keylog", line);
        if (tlsKeylogPath !== undefined)
          appendTlsKeylog(line);
        self.server?.emit?.("keylog", line, self);
      },
      error(socket, error) {
        const self = socket.data;
        if (!self)
          return;
        if (self._hadError)
          return;
        self._hadError = true;
        const callback = self[kwriteCallback];
        if (callback) {
          self[kwriteCallback] = null;
          callback(error);
        }
        self.emit("error", error);
      },
      open(socket) {
        const self = socket.data;
        if (!self)
          return;
        socket.timeout(0);
        const selfTimeout = self.timeout;
        if (selfTimeout) {
          self.setTimeout(selfTimeout);
        }
        self._handle = socket;
        self.connecting = false;
        const options = self[bunTLSConnectOptions];
        if (options) {
          const { session } = options;
          if (session) {
            self.setSession(session);
          }
        }
        if (self[kSetNoDelay]) {
          socket.setNoDelay(true);
        }
        if (self[kSetKeepAlive]) {
          socket.setKeepAlive(true, self[kSetKeepAliveInitialDelay]);
        }
        let handle;
        if (self[kSetTOS] !== undefined && (handle = self._handle)?.setTypeOfService) {
          handle.setTypeOfService(self[kSetTOS]);
        }
        if (!self[kupgraded]) {
          self[kBytesWritten] = socket.bytesWritten;
          self.emit("connect", self);
          self.emit("ready");
        }
        SocketHandlers.drain(socket);
      },
      handshake(socket, success, verifyError) {
        const { data: self } = socket;
        if (!self)
          return;
        if (!success && verifyError?.code === "ECONNRESET") {
          return;
        }
        const isProtocolFailure = !success && verifyError?.code != null && (verifyError.code === "EPROTO" || /^ERR_(SSL|OSSL)_/.test(verifyError.code));
        if (isProtocolFailure) {
          self.destroy(tlsHandshakeError(verifyError));
          return;
        }
        onClientHandshakeComplete(self, socket, verifyError);
      },
      timeout(socket) {
        const self = socket.data;
        if (!self)
          return;
        self.emit("timeout", self);
      },
      binaryType: "buffer"
    };
    function readStop(self, handle) {
      handle?.pause?.();
      if (self[kupgraded] && !(self[kupgraded] instanceof Socket))
        return;
      self[kPausedUnref] = true;
      if (!self[kwriteCallback])
        handle?.unref?.();
    }
    function pauseOnCreate(self, handle) {
      readStop(self, handle);
      self.readableFlowing = false;
    }
    function restorePausedHold(self, handle) {
      if (!self[kPausedUnref])
        return;
      self[kPausedUnref] = false;
      if (!self[kUserUnrefed])
        handle?.ref?.();
    }
    function unrefAfterDrain(self, handle) {
      if ((self[kended] || self[kPausedUnref]) && !self[kUserUnrefed] && handle === self._handle)
        handle.unref?.();
    }
    function finishSocketEnd(self) {
      if (self[kended])
        return;
      self[kended] = true;
      if (!self.allowHalfOpen)
        self.write = writeAfterFIN;
      self.push(null);
      Duplex.prototype.read.call(self, 0);
      const socket = self._handle;
      if (socket && !self[kwriteCallback]) {
        socket.unref?.();
        self[kPausedUnref] = false;
      }
    }
    function deferEndForOnreadTail(self) {
      if (self[kOnreadTail] === undefined || self.destroyed)
        return false;
      self[kOnreadPendingEnd] = true;
      return true;
    }
    function SocketEmitEndNT(self, _err) {
      const teardownNoise = self[kended] && self.writableFinished;
      if (_err && !self.destroyed && !self._hadError && !teardownNoise && self.listenerCount("error") > 0) {
        self.once("error", () => {});
        let errErrno;
        if (_err.code === undefined && typeof (errErrno = _err.errno) === "number" && errErrno !== 0) {
          const er = new ErrnoException(errErrno, "read");
          if (typeof er.code === "string" && /^E[A-Z0-9]+$/.test(er.code)) {
            self.destroy(er);
            return;
          }
        }
        if (_err.code === undefined || _err.code === "ECONNRESET") {
          const er = new ConnResetException("read ECONNRESET");
          er.errno = _err.errno ?? (process.platform === "win32" ? -4077 : process.platform === "linux" ? -104 : -54);
          er.syscall = "read";
          self.destroy(er);
        } else {
          self.destroy(_err);
        }
        return;
      }
      if (!self[kended]) {
        finishSocketEnd(self);
      } else if (_err && !self.destroyed) {
        self.destroy();
      }
      const pendingWrite = self[kwriteCallback];
      if (pendingWrite && (self.destroyed || _err)) {
        self[kwriteCallback] = null;
        pendingWrite(_err ?? codes.SOCKET_CLOSED());
      }
    }
    function toSNIError(err) {
      return err instanceof Error ? err : Object.assign(new Error("SNI callback error"), { reason: err });
    }
    function consumeSNIResult(state, err, context) {
      if (err) {
        state.failed = toSNIError(err);
        return;
      }
      if (context == null)
        return;
      const innerContext = typeof context === "object" ? context.context : undefined;
      if (innerContext) {
        state.selected = innerContext;
      } else if (state.server?.[kNativeSecureContextCtor] && context instanceof state.server[kNativeSecureContextCtor]) {
        state.selected = context;
      } else {
        state.failed = new Error("Invalid SNI context");
      }
    }
    function stashSNIError(state) {
      const target = state.socketHandle?.data ?? state.server;
      if (target)
        target[kSNIError] = state.failed;
    }
    function onSNIResolution(state, err, context) {
      if (state.settled)
        return;
      state.settled = true;
      consumeSNIResult(state, err, context);
      if (!state.suspended)
        return;
      if (state.failed !== undefined) {
        stashSNIError(state);
        state.socketHandle?.resumeSNI(undefined, true);
      } else {
        state.socketHandle?.resumeSNI(state.selected, false);
      }
    }
    const ServerHandlers = {
      data(socket, buffer) {
        const { data: self } = socket;
        if (!self)
          return;
        self._unrefTimer();
        self.bytesRead += buffer.length;
        if (!self.push(buffer)) {
          readStop(self, socket);
        }
      },
      keylog(socket, line) {
        const { data: self } = socket;
        if (!self)
          return;
        self.emit("keylog", line);
        if (tlsKeylogPath !== undefined)
          appendTlsKeylog(line);
        self.server?.emit?.("keylog", line, self);
      },
      alpnCallback(socket, servername, protocolsWire) {
        const self = socket.data;
        const server = self?.server ?? self;
        const cb = server?._ALPNCallback;
        if (typeof cb !== "function")
          return false;
        const wire = Buffer.isBuffer(protocolsWire) ? protocolsWire : Buffer.from(protocolsWire);
        const protocols = [];
        for (let i = 0;i + 1 <= wire.length; ) {
          const n = wire[i];
          protocols.push(wire.toString("latin1", i + 1, i + 1 + n));
          i += 1 + n;
        }
        let result;
        try {
          result = cb.call(self, { servername, protocols });
        } catch (err) {
          if (self)
            self[kALPNError] = err;
          return;
        }
        if (result !== undefined && !ArrayPrototypeIncludes.call(protocols, result)) {
          const err = codes.TLS_ALPN_CALLBACK_INVALID_RESULT(`ALPN callback returned a value (${result}) that did not match any of the client's offered protocols (${ArrayPrototypeJoin.call(protocols, ", ")})`);
          if (self)
            self[kALPNError] = err;
          return;
        }
        return result;
      },
      serverName(server, servername, socketHandle) {
        const cb = server?._SNICallback;
        if (typeof cb !== "function" || !servername)
          return;
        const state = {
          server,
          socketHandle,
          selected: undefined,
          failed: undefined,
          settled: false,
          suspended: false
        };
        try {
          cb.call(server, servername, onSNIResolution.bind(null, state));
        } catch (err) {
          state.settled = true;
          state.failed = toSNIError(err);
        }
        if (!state.settled) {
          if (!socketHandle)
            return;
          state.suspended = true;
          return true;
        }
        const failed = state.failed;
        if (failed !== undefined) {
          stashSNIError(state);
          return failed;
        }
        return state.selected;
      },
      close(socket, err) {
        debug("Bun.Server close");
        const data = this.data;
        if (!data)
          return;
        {
          if (!data[kclosed]) {
            data[kclosed] = true;
            detachSocket(data);
            SocketEmitEndNT(data, err);
            data.data = null;
            socket[owner_symbol] = null;
          }
        }
      },
      end(socket) {
        SocketHandlers.end(socket);
      },
      open(socket) {
        debug("Bun.Server open");
        const self = socket.data;
        if (!self)
          return;
        const handle = self._handle || socket.listener;
        if (handle && typeof handle.onconnection === "function") {
          handle.onconnection(0, socket);
        }
      },
      handshake(socket, success, verifyError) {
        const self = socket.data;
        const server = self.server;
        if (self[khandshakeTimer]) {
          clearTimeout(self[khandshakeTimer]);
          self[khandshakeTimer] = undefined;
          self.removeListener("timeout", onHandshakeTimeout);
        }
        if (!success) {
          let alreadyDestroyed;
          if (self._hadError || self[kerrorEmitted] || (alreadyDestroyed = self.destroyed)) {
            if (!(alreadyDestroyed ?? self.destroyed))
              self.destroy();
            return;
          }
          let err;
          if (self[kSNIError]) {
            err = self[kSNIError];
            self[kSNIError] = undefined;
          } else if (server?.[kSNIError]) {
            err = server[kSNIError];
            server[kSNIError] = undefined;
          } else if (self[kALPNError]) {
            err = self[kALPNError];
            self[kALPNError] = undefined;
          } else {
            err = tlsHandshakeError(verifyError);
          }
          self.servername = socket.getServername();
          self._hadError = true;
          self._closeAfterHandlingError = true;
          self.destroy(err);
          return;
        }
        self._securePending = false;
        self.secureConnecting = false;
        self._secureEstablished = !!success;
        self.servername = socket.getServername();
        self.alpnProtocol = socket.alpnProtocol;
        self[kVerifyError] = verifyError ?? null;
        if (self._requestCert) {
          if (verifyError) {
            self.authorized = false;
            self.authorizationError = verifyError.code || verifyError.message;
            if (self._rejectUnauthorized) {
              self[kerrorEmitted] = true;
              server?.emit("tlsClientError", verifyError, self);
              self.destroy(verifyError);
              return;
            }
          } else {
            self.authorized = true;
          }
        }
        const pauseOnConnect = server?.pauseOnConnect;
        if (pauseOnConnect) {
          pauseOnCreate(self, socket);
        }
        if (!pauseOnConnect && !self.destroyed) {
          self.read(0);
        }
        try {
          if (server) {
            if (!self.destroyed && self._releaseControl()) {
              server.emit("secureConnection", self);
            }
          }
          if (self.destroyed)
            return;
          self.emit(kSecureConnectDone);
          if (!server)
            self.emit("secure", self);
        } catch (err) {
          reportError(err);
        }
      },
      error(socket, error) {
        const data = this.data;
        if (!data)
          return;
        if (data._hadError)
          return;
        data._hadError = true;
        const bunTLS = data[bunTlsSymbol];
        if (typeof bunTLS === "function") {
          const callback = data[kwriteCallback];
          if (callback) {
            data[kwriteCallback] = null;
            callback(error);
          }
          if (!data._secureEstablished) {
            data._closeAfterHandlingError = true;
            data.destroy(error);
          } else if (data.isServer && data._rejectUnauthorized && /peer did not return a certificate/.test(error?.message)) {
            data.destroy();
          } else {
            data.emit("_tlsError", error);
            data.destroy(data.listenerCount("error") > 0 ? error : undefined);
          }
          return;
        }
        const callback = data[kwriteCallback];
        if (callback) {
          data[kwriteCallback] = null;
          callback(error);
        }
        if (!data.destroyed) {
          data.destroy(error);
        }
      },
      timeout(socket) {
        SocketHandlers.timeout(socket);
      },
      drain(socket) {
        SocketHandlers.drain(socket);
      },
      binaryType: "buffer"
    };
    function applyRejectUnauthorized(self, tls, rejectUnauthorized) {
      if (typeof rejectUnauthorized !== "undefined") {
        self._rejectUnauthorized = rejectUnauthorized !== false;
        tls.rejectUnauthorized = self._rejectUnauthorized;
      } else {
        self._rejectUnauthorized = tls.rejectUnauthorized;
      }
    }
    function onSocketTLSError(err) {
      if (!this._controlReleased && !this[kerrorEmitted]) {
        this[kerrorEmitted] = true;
        this.server?.emit("tlsClientError", err, this);
      }
    }
    function onHandshakeTimeout() {
      this._emitTLSError(codes.TLS_HANDSHAKE_TIMEOUT());
    }
    function onHandshakeDeadline(socket) {
      socket[khandshakeTimer] = undefined;
      if (!socket.destroyed)
        socket.emit("timeout");
    }
    function clearHandshakeTimeout() {
      if (this[khandshakeTimer]) {
        clearTimeout(this[khandshakeTimer]);
        this[khandshakeTimer] = undefined;
        this.removeListener("timeout", onHandshakeTimeout);
      }
    }
    function initAcceptedTLSSocket(server, socket) {
      socket[kerrorEmitted] = false;
      socket.on("_tlsError", onSocketTLSError);
      socket.on("error", onSocketTLSError);
      const handshakeTimeout = server._handshakeTimeout;
      if (!(handshakeTimeout > 0))
        return;
      socket.once("timeout", onHandshakeTimeout);
      const timer = setTimeout(onHandshakeDeadline, handshakeTimeout, socket);
      timer.unref?.();
      socket[khandshakeTimer] = timer;
      socket.once("close", clearHandshakeTimeout);
    }
    function onconnection(err, clientHandle) {
      const handle = this;
      const self = handle[owner_symbol];
      if (err) {
        self.emit("error", err);
        return;
      }
      clientHandle[kServerSocket] = handle;
      const options = self[bunSocketServerOptions];
      const { [kSocketClass]: SClass } = options;
      const pauseOnConnect = self.pauseOnConnect;
      const _socket = new SClass({
        allowHalfOpen: self.allowHalfOpen,
        highWaterMark: self.highWaterMark
      });
      _socket.isServer = true;
      _socket._requestCert = self._requestCert;
      _socket._rejectUnauthorized = self._rejectUnauthorized;
      _socket[kAttach](clientHandle.localPort, clientHandle);
      const blockList = self.blockList;
      if (blockList) {
        const addressType = isIP(clientHandle.remoteAddress);
        if (addressType && blockList.check(clientHandle.remoteAddress, `ipv${addressType}`)) {
          const data = {
            localAddress: _socket.localAddress,
            localPort: _socket.localPort || clientHandle.localPort,
            localFamily: _socket.localFamily,
            remoteAddress: _socket.remoteAddress,
            remotePort: _socket.remotePort,
            remoteFamily: _socket.remoteFamily || "IPv4"
          };
          clientHandle.end();
          self.emit("drop", data);
          return;
        }
      }
      if (self.maxConnections != null && self._connections >= self.maxConnections) {
        const data = {
          localAddress: _socket.localAddress,
          localPort: _socket.localPort || clientHandle.localPort,
          localFamily: _socket.localFamily,
          remoteAddress: _socket.remoteAddress,
          remotePort: _socket.remotePort,
          remoteFamily: _socket.remoteFamily || "IPv4"
        };
        clientHandle.end();
        self.emit("drop", data);
        return;
      }
      const bunTLS = _socket[bunTlsSymbol];
      const isTLS = typeof bunTLS === "function";
      if (self.noDelay && clientHandle.setNoDelay) {
        _socket[kSetNoDelay] = true;
        clientHandle.setNoDelay(true);
      }
      if (self.keepAlive && clientHandle.setKeepAlive) {
        _socket[kSetKeepAlive] = true;
        _socket[kSetKeepAliveInitialDelay] = self.keepAliveInitialDelay;
        clientHandle.setKeepAlive(true, self.keepAliveInitialDelay);
      }
      self._connections++;
      _socket.server = self;
      _socket._server = self;
      if (pauseOnConnect && !isTLS) {
        pauseOnCreate(_socket, clientHandle);
      }
      if (isTLS)
        initAcceptedTLSSocket(self, _socket);
      self.emit("connection", _socket);
      if (!pauseOnConnect && !isTLS) {
        _socket.read(0);
      }
    }
    const SocketHandlers2 = {
      open(socket) {
        debug("Bun.Socket open");
        let { self, req } = socket.data;
        socket[owner_symbol] = self;
        debug("self[kupgraded]", String(self[kupgraded]));
        {
          const options = self[bunTLSConnectOptions];
          if (options) {
            const { session } = options;
            if (session) {
              self.setSession(session);
            }
          }
        }
        if (!self[kupgraded])
          req.oncomplete(0, self._handle, req, true, true);
        socket.data.req = undefined;
        if (self[kupgraded]) {
          self.connecting = false;
          SocketHandlers2.drain(socket);
        }
      },
      data(socket, buffer) {
        debug("Bun.Socket data");
        const { self } = socket.data;
        self._unrefTimer();
        self.bytesRead += buffer.length;
        if (!self.push(buffer))
          readStop(self, socket);
      },
      drain(socket) {
        debug("Bun.Socket drain");
        const { self } = socket.data;
        const callback = self[kwriteCallback];
        self.connecting = false;
        if (callback) {
          const writeChunk = self._pendingData;
          const res = socketInternals.write(socket, writeChunk || "", self._pendingEncoding || "utf8");
          if (res < 0) {
            self[kBytesWritten] = socket.bytesWritten;
            failWrite(self, res, callback);
          } else if (res) {
            self[kBytesWritten] = socket.bytesWritten;
            self._pendingData = self[kwriteCallback] = null;
            unrefAfterDrain(self, socket);
            callback(null);
          } else {
            self[kBytesWritten] = socket.bytesWritten;
            self._pendingData = null;
          }
        }
      },
      end(socket) {
        debug("Bun.Socket end");
        const { self } = socket.data;
        if (deferEndForOnreadTail(self))
          return;
        finishSocketEnd(self);
      },
      session(socket, session) {
        const { self } = socket.data;
        if (!self)
          return;
        if (self._secureEstablished) {
          self.emit("session", session);
        } else {
          self[kpendingSession] = session;
        }
      },
      keylog(socket, line) {
        const { self } = socket.data;
        if (!self)
          return;
        self.emit("keylog", line);
        if (tlsKeylogPath !== undefined)
          appendTlsKeylog(line);
      },
      close(socket, err) {
        debug("Bun.Socket close");
        let { self } = socket.data;
        if (err)
          debug(err);
        if (self[kclosed])
          return;
        self[kclosed] = true;
        if (err && !self.destroyed && socket === self._handle && self.listenerCount("error") > 0) {
          self.once("error", () => {});
          if (err.code === undefined || err.code === "ECONNRESET") {
            const er = new ConnResetException("read ECONNRESET");
            er.errno = err.errno;
            er.syscall = "read";
            self.destroy(er);
          } else {
            self.destroy(err);
          }
          return;
        }
        if (!deferEndForOnreadTail(self))
          finishSocketEnd(self);
        const pendingWrite = self[kwriteCallback];
        if (pendingWrite) {
          self[kwriteCallback] = null;
          pendingWrite(codes.SOCKET_CLOSED());
        }
      },
      handshake(socket, success, verifyError) {
        debug("Bun.Socket handshake");
        const { self } = socket.data;
        if (!success && verifyError?.code === "ECONNRESET") {
          return;
        }
        const isProtocolFailure = !success && verifyError?.code != null && (verifyError.code === "EPROTO" || /^ERR_(SSL|OSSL)_/.test(verifyError.code));
        if (isProtocolFailure) {
          self.destroy(tlsHandshakeError(verifyError));
          return;
        }
        onClientHandshakeComplete(self, socket, verifyError);
      },
      error(socket, error) {
        debug("Bun.Socket error");
        if (socket.data === undefined)
          return;
        const { self } = socket.data;
        if (self._hadError)
          return;
        self._hadError = true;
        const callback = self[kwriteCallback];
        if (callback) {
          self[kwriteCallback] = null;
          callback(error);
        }
        if (!self.destroyed)
          process.nextTick(destroyNT, self, error);
      },
      timeout(socket) {
        debug("Bun.Socket timeout");
        const { self } = socket.data;
        self.emit("timeout", self);
      },
      connectError(socket, error) {
        debug("Bun.Socket connectError");
        let { self, req } = socket.data;
        socket[owner_symbol] = self;
        socket.data.req = undefined;
        if (req.dispatching) {
          req.errno = error.errno || uv().UV_ECANCELED;
          return;
        }
        req.oncomplete(error.errno, self._handle, req, true, true);
      }
    };
    const { serverName: _serverNameHandler, alpnCallback: _alpnCallbackHandler, ...ServerHandlersNoSNI } = ServerHandlers;
    const { serverName: _snOnly, ...ServerHandlersALPNOnly } = ServerHandlers;
    const { alpnCallback: _acOnly, ...ServerHandlersSNIOnly } = ServerHandlers;
    function serverHandlersFor(server) {
      const sni = !!server._SNICallback;
      const alpn = !!server._ALPNCallback;
      if (sni && alpn)
        return ServerHandlers;
      if (sni)
        return ServerHandlersSNIOnly;
      if (alpn)
        return ServerHandlersALPNOnly;
      return ServerHandlersNoSNI;
    }
    const kNetTraceCat = "node,node.net,node.net.native";
    const kTraceConnectActive = Symbol("kTraceConnectActive");
    let traceEvents = null;
    function traceConnectStart(req, pipePath) {
      traceEvents ??= require("internal/trace_events");
      if (!traceEvents.isCategoryGroupEnabled(kNetTraceCat))
        return;
      if (pipePath !== undefined) {
        const isAbstract = pipePath.charCodeAt(0) === 0;
        traceEvents.emitEventWithArgs("b", kNetTraceCat, "connect", undefined, {
          path_type: isAbstract ? "abstract socket" : "file",
          pipe_path: isAbstract ? pipePath.slice(1) : pipePath
        });
      } else {
        traceEvents.emitEvent("b", kNetTraceCat, "connect");
      }
      req[kTraceConnectActive] = true;
    }
    function traceConnectEnd(req) {
      if (req && req[kTraceConnectActive]) {
        req[kTraceConnectActive] = false;
        traceEvents.emitEvent("e", kNetTraceCat, "connect");
      }
    }
    function kConnectTcp(self, addressType, req, address, port) {
      debug("SocketHandle.kConnectTcp", addressType, address, port);
      return kConnectDispatch(self, req, {
        hostname: address,
        port,
        localAddress: req.localAddress || undefined,
        localPort: req.localPort || undefined,
        ipv6Only: addressType === 6,
        allowHalfOpen: true,
        tls: req.tls,
        pauseOnConnect: req.pauseOnConnect,
        data: { self, req },
        socket: self[khandlers]
      });
    }
    function kConnectPipe(self, req, address) {
      debug("SocketHandle.kConnectPipe");
      return kConnectDispatch(self, req, {
        hostname: address,
        unix: address,
        allowHalfOpen: true,
        tls: req.tls,
        pauseOnConnect: req.pauseOnConnect,
        data: { self, req },
        socket: self[khandlers]
      });
    }
    function kConnectDispatch(self, req, opts) {
      req.dispatching = true;
      const promise = doConnect(self._handle, opts);
      req.dispatching = false;
      promise.catch((_reason) => {
        traceConnectEnd(req);
      });
      const errno = req.errno;
      if (errno !== undefined) {
        req.errno = undefined;
        return errno;
      }
      return 0;
    }
    function Socket(options) {
      if (!(this instanceof Socket))
        return new Socket(options);
      let {
        socket,
        signal,
        allowHalfOpen = false,
        onread = null,
        noDelay = false,
        keepAlive = false,
        keepAliveInitialDelay,
        ...opts
      } = options || {};
      if (options?.objectMode)
        throw codes.INVALID_ARG_VALUE("options.objectMode", options.objectMode, "is not supported");
      if (options?.readableObjectMode)
        throw codes.INVALID_ARG_VALUE("options.readableObjectMode", options.readableObjectMode, "is not supported");
      if (options?.writableObjectMode)
        throw codes.INVALID_ARG_VALUE("options.writableObjectMode", options.writableObjectMode, "is not supported");
      if (keepAliveInitialDelay !== undefined) {
        validateNumber(keepAliveInitialDelay, "options.keepAliveInitialDelay");
        if (keepAliveInitialDelay < 0)
          keepAliveInitialDelay = 0;
      }
      if (options?.fd !== undefined) {
        validateInt32(options.fd, "options.fd", 0);
      }
      Duplex.call(this, {
        ...opts,
        allowHalfOpen,
        readable: true,
        writable: true,
        emitClose: false,
        autoDestroy: true,
        decodeStrings: false
      });
      this._parent = null;
      this._parentWrap = null;
      this[kupgraded] = null;
      this[kSetNoDelay] = Boolean(noDelay);
      this[kSetKeepAlive] = Boolean(keepAlive);
      this[kSetKeepAliveInitialDelay] = MathMax(0, ~~keepAliveInitialDelay);
      this[khandlers] = SocketHandlers2;
      this.bytesRead = 0;
      this[kBytesWritten] = undefined;
      this[kclosed] = false;
      this[kended] = false;
      this.connecting = false;
      this._host = undefined;
      this._port = undefined;
      this[bunTLSConnectOptions] = null;
      this.timeout = 0;
      this[kTimeout] = null;
      this[kwriteCallback] = undefined;
      this._pendingData = undefined;
      this._pendingEncoding = undefined;
      this._hadError = false;
      this.isServer = false;
      this._handle = options?.handle || null;
      this[ksocket] = undefined;
      this.server = undefined;
      this._server = undefined;
      this._peername = null;
      this._sockname = null;
      this._closeAfterHandlingError = false;
      this.on("end", onSocketEnd);
      if (options?.fd !== undefined) {
        const { fd } = options;
        validateInt32(fd, "fd", 0);
        if (options.writable === true) {
          let stats;
          try {
            stats = require("node:fs").fstatSync(fd);
          } catch {
            throw codes.INVALID_FD_TYPE("UNKNOWN");
          }
          const optionsReadable = options.readable;
          if (stats.isFIFO() || stats.isCharacterDevice() || stats.isFile() || stats.isSocket() && optionsReadable !== true) {
            this[kSyncWriteFd] = fd;
            this._write = fdSyncWrite;
            this._writev = fdSyncWritev;
            if (optionsReadable !== true) {
              this.push(null);
              this.read(0);
            }
          }
        }
      }
      if (socket instanceof Socket) {
        this[ksocket] = socket;
      }
      if (onread) {
        if (typeof onread !== "object") {
          throw new TypeError("onread must be an object");
        }
        if (typeof onread.callback !== "function") {
          throw new TypeError("onread.callback must be a function");
        }
        if (!isUint8Array(onread.buffer) && typeof onread.buffer !== "function")
          onread = null;
      }
      if (onread) {
        const onreadBuffer = onread.buffer;
        const onreadCallback = onread.callback;
        const onreadBufferIsFn = typeof onreadBuffer === "function";
        const self = this;
        this[kOnreadTail] = undefined;
        this[kOnreadDraining] = false;
        if (onreadBufferIsFn) {
          const first = onreadBuffer();
          this[kOnreadBuffer] = isUint8Array(first) ? first : true;
        } else {
          this[kOnreadBuffer] = onreadBuffer;
        }
        this[kOnreadDeliver] = function deliver(buffer) {
          let offset = 0;
          const total = buffer.length;
          while (offset < total) {
            const dest = self[kOnreadBuffer];
            if (dest === true) {
              let ret;
              try {
                ret = onreadCallback(total - offset, true);
                if (onreadBufferIsFn) {
                  const next = onreadBuffer();
                  if (isUint8Array(next))
                    self[kOnreadBuffer] = next;
                }
              } catch (e) {
                reportError(e);
              }
              if (self.destroyed)
                return;
              if (ret === false || self.isPaused()) {
                self[kOnreadTail] = kOnreadEmptyTail;
                readStop(self, self._handle);
              }
              return;
            }
            if (dest.length === 0) {
              const err = new Error("read ENOBUFS");
              err.code = "ENOBUFS";
              err.errno = uv().UV_ENOBUFS;
              err.syscall = "read";
              self.destroy(err);
              return;
            }
            const n = MathMin(dest.length, total - offset);
            dest.set(buffer.subarray(offset, offset + n));
            offset += n;
            let ret;
            try {
              ret = onreadCallback(n, dest);
              if (onreadBufferIsFn) {
                const next = onreadBuffer();
                if (isUint8Array(next))
                  self[kOnreadBuffer] = next;
              }
            } catch (e) {
              reportError(e);
            }
            if (self.destroyed)
              return;
            if (ret === false || self.isPaused()) {
              const rest = buffer.subarray(offset);
              self[kOnreadTail] = rest.length !== 0 ? rest : kOnreadEmptyTail;
              readStop(self, self._handle);
              return;
            }
          }
        };
        this[khandlers] = {
          ...SocketHandlers2,
          data(socket, buffer) {
            const { self } = socket.data;
            if (!self)
              return;
            self._unrefTimer();
            const tail = self[kOnreadTail];
            if (tail !== undefined) {
              self[kOnreadTail] = Buffer.concat([tail, buffer]);
              return;
            }
            self[kOnreadDeliver](buffer);
          }
        };
      }
      if (signal) {
        if (signal.aborted) {
          process.nextTick(destroyNT, this, makeAbortError(undefined, { cause: signal.reason }));
        } else {
          addAbortListener ??= require("internal/abort_listener").addAbortListener;
          const disposable = addAbortListener(signal, destroyWhenAborted.bind(this));
          this.once("close", disposable[Symbol.dispose]);
        }
      }
      const optsBlockList = opts.blockList;
      if (optsBlockList) {
        if (!lazyBlockList().isBlockList(optsBlockList)) {
          throw codes.INVALID_ARG_TYPE("options.blockList", "net.BlockList", optsBlockList);
        }
        this.blockList = optsBlockList;
      }
    }
    toClass(Socket, "Socket", Duplex);
    Socket.prototype.address = function address() {
      return {
        address: this.localAddress,
        family: this.localFamily,
        port: this.localPort
      };
    };
    Socket.prototype._onTimeout = function() {
      if (this._pendingData) {
        return;
      }
      const handle = this._handle;
      if (handle && getBufferedAmount(handle) > 0) {
        return;
      }
      this.emit("timeout");
    };
    Object.defineProperty(Socket.prototype, "bufferSize", {
      get: function() {
        if (this._handle) {
          return this.writableLength;
        }
      }
    });
    Object.defineProperty(Socket.prototype, "_bytesDispatched", {
      get: function() {
        return this[kBytesWritten] || 0;
      }
    });
    Object.defineProperty(Socket.prototype, "bytesWritten", {
      get: function() {
        let bytes = this[kBytesWritten] || 0;
        const data = this._pendingData;
        const writableBuffer = this.writableBuffer;
        if (!writableBuffer)
          return;
        for (const el of writableBuffer) {
          bytes += el.chunk instanceof Buffer ? el.chunk.length : Buffer.byteLength(el.chunk, el.encoding);
        }
        if (Array.isArray(data)) {
          for (let i = 0;i < data.length; i++) {
            const chunk = data[i];
            if (data.allBuffers || chunk instanceof Buffer)
              bytes += chunk.length;
            else
              bytes += Buffer.byteLength(chunk.chunk, chunk.encoding);
          }
        } else if (data) {
          if (typeof data !== "string")
            bytes += data.length;
          else
            bytes += Buffer.byteLength(data, this._pendingEncoding || "utf8");
        }
        return bytes;
      }
    });
    Socket.prototype[kAttach] = function(port, socket) {
      socket.data = this;
      socket[owner_symbol] = this;
      const timeout = this.timeout;
      if (timeout) {
        this.setTimeout(timeout);
      }
      socket.timeout(0);
      this._handle = socket;
      this.connecting = false;
      if (this[kSetNoDelay]) {
        socket.setNoDelay(true);
      }
      if (this[kSetKeepAlive]) {
        socket.setKeepAlive(true, this[kSetKeepAliveInitialDelay]);
      }
      if (!this[kupgraded]) {
        this[kBytesWritten] = socket.bytesWritten;
        this.emit("connect", this);
        this.emit("ready");
      }
      SocketHandlers.drain(socket);
    };
    Socket.prototype[kCloseRawConnection] = function() {
      const connection = this[kupgraded];
      connection.connecting = false;
      connection._handle = null;
      connection.unref();
      connection.destroy();
    };
    Socket.prototype.connect = function connect(...args) {
      debug("Socket.prototype.connect");
      {
        const [options, connectListener] = Array.isArray(args[0]) && args[0][normalizedArgsSymbol] ? args[0] : normalizeArgs(args);
        let connection = this[ksocket];
        let upgradeDuplex = false;
        let { port, host, path, socket, rejectUnauthorized, checkServerIdentity, session, fd, pauseOnConnect } = options;
        this.servername = options.servername;
        if (socket) {
          connection = socket;
        }
        if (fd != null) {
          doConnect(this._handle, {
            data: this,
            fd,
            ...options.fdIsRawSocket === true ? { fdIsRawSocket: true } : {},
            socket: SocketHandlers,
            allowHalfOpen: true,
            pauseOnConnect
          }).catch((error) => {
            if (!this.destroyed) {
              this.emit("error", error);
              this.emit("close", true);
            }
          });
        }
        if (pauseOnConnect) {
          if (fd != null)
            pauseOnCreate(this, this._handle);
          else
            this.readableFlowing = false;
        } else {
          process.nextTick(() => {
            if (!this.connecting && !this.isPaused())
              this.read(0);
          });
          if (fd == null)
            this.connecting = true;
        }
        if (fd != null) {
          return this;
        }
        if (!(socket && isObject(socket) && socket instanceof Duplex) && port === undefined && path == null) {
          throw codes.MISSING_ARGS(["options", "port", "path"]);
        }
        const bunTLS = this[bunTlsSymbol];
        var tls = undefined;
        if (typeof bunTLS === "function") {
          tls = bunTLS.call(this, port, host, true);
          this._requestCert = true;
          if (tls) {
            applyRejectUnauthorized(this, tls, rejectUnauthorized);
            tls.requestCert = true;
            tls.session = session || tls.session;
            this.servername = tls.servername;
            if (checkServerIdentity !== undefined) {
              validateFunction(checkServerIdentity, "options.checkServerIdentity");
            }
            tls.checkServerIdentity = checkServerIdentity || tls.checkServerIdentity;
            this[bunTLSConnectOptions] = tls;
            let tlsSocket;
            if (!connection && (tlsSocket = tls.socket)) {
              connection = tlsSocket;
            }
          }
          if (connection) {
            if (typeof connection !== "object" || !(connection instanceof Socket) || typeof connection[bunTlsSymbol] === "function") {
              if (connection instanceof Duplex) {
                upgradeDuplex = true;
              } else {
                throw new TypeError("socket must be an instance of net.Socket or Duplex");
              }
            }
          }
          this.authorized = false;
          this.secureConnecting = true;
          this._secureEstablished = false;
          this._securePending = true;
          this[kConnectOptions] = options;
          this.prependListener("end", onConnectEnd);
        }
        if (connection) {
          if (!(connection instanceof Socket)) {
            this.connecting = false;
          }
          if (connectListener != null)
            this.once("secureConnect", connectListener);
          try {
            this._undestroy();
            const socket = connection._handle;
            if (!upgradeDuplex && socket) {
              upgradeDuplex = isNamedPipeSocket(socket) || hasUnflushedWrites(connection);
            }
            if (upgradeDuplex) {
              this[kupgraded] = connection;
              const [result, events] = upgradeDuplexToTLS(connection, {
                data: { self: this, req: { oncomplete: afterConnect } },
                tls,
                socket: this[khandlers]
              });
              connection.on("data", events[0]);
              connection.on("end", events[1]);
              connection.on("drain", events[2]);
              connection.on("close", events[3]);
              this._handle = result;
            } else {
              if (socket && !connection.connecting) {
                this[kupgraded] = connection;
                const result = upgradeTLSDeferred(socket, {
                  data: { self: this, req: { oncomplete: afterConnect } },
                  tls,
                  socket: this[khandlers],
                  isServer: false
                });
                if (result) {
                  const [raw, tls] = result;
                  connection._handle = raw;
                  raw[kAdoptedTLSRaw] = true;
                  this.once("end", this[kCloseRawConnection]);
                  raw.connecting = false;
                  this._handle = tls;
                } else {
                  this._handle = null;
                  throw new Error("Invalid socket");
                }
              } else {
                connection.once("connect", () => {
                  if (this.destroyed) {
                    connection.destroy();
                    return;
                  }
                  const socket = connection._handle;
                  if (!upgradeDuplex && socket) {
                    upgradeDuplex = isNamedPipeSocket(socket) || hasUnflushedWrites(connection);
                  }
                  if (upgradeDuplex) {
                    this[kupgraded] = connection;
                    const [result, events] = upgradeDuplexToTLS(connection, {
                      data: { self: this, req: { oncomplete: afterConnect } },
                      tls,
                      socket: this[khandlers]
                    });
                    connection.on("data", events[0]);
                    connection.on("end", events[1]);
                    connection.on("drain", events[2]);
                    connection.on("close", events[3]);
                    this._handle = result;
                  } else {
                    this[kupgraded] = connection;
                    const result = upgradeTLSDeferred(socket, {
                      data: { self: this, req: { oncomplete: afterConnect } },
                      tls,
                      socket: this[khandlers],
                      isServer: false
                    });
                    if (result) {
                      const [raw, tls] = result;
                      connection._handle = raw;
                      raw[kAdoptedTLSRaw] = true;
                      this.once("end", this[kCloseRawConnection]);
                      raw.connecting = false;
                      this._handle = tls;
                    } else {
                      this._handle = null;
                      throw new Error("Invalid socket");
                    }
                  }
                });
              }
            }
          } catch (error) {
            process.nextTick(emitErrorAndCloseNextTick, this, error);
          }
          return this;
        }
      }
      const [options, cb] = Array.isArray(args[0]) && args[0][normalizedArgsSymbol] ? args[0] : normalizeArgs(args);
      if (typeof this[bunTlsSymbol] === "function" && cb !== null) {
        this.once("secureConnect", cb);
      } else if (cb !== null) {
        this.once("connect", cb);
      }
      if (this._parent?.connecting) {
        return this;
      }
      const socketWrite = Socket.prototype.write;
      if (this.write !== socketWrite) {
        this.write = socketWrite;
      }
      if (this.destroyed) {
        this._handle = null;
        this._peername = null;
        this._sockname = null;
      }
      this.connecting = true;
      const { path } = options;
      const pipe = !!path;
      debug("pipe", pipe, path);
      if (!this._handle) {
        this._handle = newDetachedSocket(typeof this[bunTlsSymbol] === "function");
        initSocketHandle(this);
      }
      if (!pipe) {
        lookupAndConnect(this, options);
      } else {
        validateString(path, "options.path");
        internalConnect(this, options, path);
      }
      return this;
    };
    Socket.prototype[kReinitializeHandle] = function reinitializeHandle(handle) {
      this._handle?.close();
      this._handle = handle;
      this._handle[owner_symbol] = this;
      initSocketHandle(this);
    };
    Socket.prototype.end = function end(data, encoding, callback) {
      debug("Socket.prototype.end");
      return Duplex.prototype.end.call(this, data, encoding, callback);
    };
    Socket.prototype._destroy = function _destroy(err, callback) {
      debug("Socket.prototype._destroy");
      this.connecting = false;
      const upgraded = this[kupgraded];
      if (upgraded && !(upgraded instanceof Socket) && !upgraded.destroyed) {
        upgraded.destroy?.();
      }
      const syncFd = this[kSyncWriteFd];
      if (syncFd !== undefined) {
        this[kSyncWriteFd] = undefined;
        delete this._write;
        delete this._writev;
        if (syncFd > 2) {
          try {
            require("node:fs").closeSync(syncFd);
          } catch {}
        }
      }
      for (let s = this;s !== null; s = s._parent) {
        clearTimeout(s[kTimeout]);
      }
      debug("close");
      if (this._handle) {
        debug("close handle");
        const isException = err ? true : false;
        this[kBytesWritten] = this._handle.bytesWritten;
        const currentHandle = this._handle;
        if (this.resetAndClosing) {
          this.resetAndClosing = false;
          const err = this._handle.terminate();
          setImmediate(() => {
            debug("emit close");
            this.emit("close", isException);
          });
          if (err)
            this.emit("error", new ErrnoException(err, "reset"));
        } else if (this._closeAfterHandlingError) {
          queueMicrotask(() => closeSocketHandle(this, isException, true));
        } else if (currentHandle[kAdoptedTLSRaw]) {
          currentHandle.pause?.();
          setImmediate(closeAdoptedTLSRawNT, currentHandle, this, isException);
        } else {
          closeSocketHandle(this, isException);
        }
        if (!this._closeAfterHandlingError) {
          const handle = this._handle;
          if (handle)
            handle.onread = noop;
          this._handle = null;
          this._sockname = null;
        }
        callback(err);
      } else {
        callback(err);
        process.nextTick(emitCloseNT, this, err ? true : false);
      }
      const server = this._server;
      if (server) {
        debug("has server");
        server._connections--;
        if (server._emitCloseIfDrained) {
          server._emitCloseIfDrained();
        }
      }
    };
    Socket.prototype._final = function _final(callback) {
      debug("Socket.prototype._final");
      if (this.connecting) {
        return this.once("connect", this._final.bind(this, callback));
      }
      const socket = this._handle;
      if (!socket)
        return callback();
      process.nextTick(endNT, socket, callback);
    };
    Object.defineProperty(Socket.prototype, "localAddress", {
      get: function() {
        return this._getsockname().address;
      }
    });
    Object.defineProperty(Socket.prototype, "localFamily", {
      get: function() {
        return this._getsockname().family;
      }
    });
    Object.defineProperty(Socket.prototype, "localPort", {
      get: function() {
        return this._getsockname().port;
      }
    });
    Object.defineProperty(Socket.prototype, "_connecting", {
      get: function() {
        return this.connecting;
      }
    });
    Object.defineProperty(Socket.prototype, "pending", {
      get: function() {
        return !this._handle || this.connecting;
      }
    });
    function hasUnflushedWrites(connection) {
      return connection.writableLength > 0 || connection[kwriteCallback] != null;
    }
    function drainOnreadTail(self, fromRead) {
      if (self[kOnreadTail] === undefined)
        return false;
      if (fromRead)
        self[kOnreadReadRequested] = true;
      if (!self[kOnreadDraining]) {
        self[kOnreadDraining] = true;
        process.nextTick(drainOnreadTailNT, self);
      }
      return true;
    }
    function drainOnreadTailNT(socket) {
      socket[kOnreadDraining] = false;
      const fromRead = socket[kOnreadReadRequested];
      socket[kOnreadReadRequested] = false;
      const tail = socket[kOnreadTail];
      if (tail === undefined || socket.destroyed)
        return;
      if (!fromRead && socket.isPaused())
        return;
      socket[kOnreadTail] = undefined;
      socket[kOnreadDeliver](tail);
      if (socket[kOnreadTail] !== undefined || socket.destroyed)
        return;
      if (socket[kOnreadPendingEnd]) {
        socket[kOnreadPendingEnd] = false;
        finishSocketEnd(socket);
      } else if (fromRead || !socket.isPaused()) {
        socket._handle?.resume?.();
        restorePausedHold(socket, socket._handle);
      }
    }
    Socket.prototype.resume = function resume() {
      const ret = Duplex.prototype.resume.call(this);
      if (!this.connecting && !drainOnreadTail(this)) {
        this._handle?.resume?.();
      }
      restorePausedHold(this, this._handle);
      return ret;
    };
    Socket.prototype.pause = function pause() {
      const handle = this._handle;
      if (handle && this[kOnreadBuffer] !== undefined && !this.connecting && !this.destroyed) {
        readStop(this, handle);
      }
      return Duplex.prototype.pause.call(this);
    };
    Socket.prototype[Symbol.for("::bunUpgradeServerTLS::")] = function(connection, tls) {
      const socket = connection._handle;
      if (!socket || connection.encrypted || hasUnflushedWrites(connection)) {
        const [result, events] = upgradeDuplexToTLS(connection, {
          data: this,
          tls,
          socket: serverHandlersFor(this),
          isServer: true
        });
        connection.on("data", events[0]);
        connection.on("end", events[1]);
        connection.on("drain", events[2]);
        connection.on("close", events[3]);
        this[kupgraded] = connection;
        this._handle = result;
        return;
      }
      this[kupgraded] = connection;
      process.nextTick(() => {
        if (this.destroyed || connection.destroyed) {
          this.destroy();
          return;
        }
        const handle = connection._handle;
        if (!handle) {
          this.destroy();
          return;
        }
        if (hasUnflushedWrites(connection)) {
          const [result, events] = upgradeDuplexToTLS(connection, {
            data: this,
            tls,
            socket: serverHandlersFor(this),
            isServer: true
          });
          connection.on("data", events[0]);
          connection.on("end", events[1]);
          connection.on("drain", events[2]);
          connection.on("close", events[3]);
          this._handle = result;
          this.emit(kUpgradeAttached);
          return;
        }
        const pending = connection.read();
        const result = handle.upgradeTLS({
          data: this,
          tls,
          socket: serverHandlersFor(this),
          isServer: true,
          initialData: pending || undefined
        });
        if (!result) {
          this._handle = null;
          this.destroy(new Error("Invalid socket"));
          return;
        }
        const [raw, tlsHandle] = result;
        connection._handle = raw;
        raw[kAdoptedTLSRaw] = true;
        this.once("end", this[kCloseRawConnection]);
        raw.connecting = false;
        this._handle = tlsHandle;
        this.emit(kUpgradeAttached);
      });
    };
    Socket.prototype.read = function read(size) {
      if (!this.connecting && !drainOnreadTail(this, true)) {
        this._handle?.resume?.();
        restorePausedHold(this, this._handle);
      }
      return Duplex.prototype.read.call(this, size);
    };
    Socket.prototype._read = function _read(size) {
      const socket = this._handle;
      if (this.connecting || !socket) {
        this.once("connect", () => this._read(size));
      } else if (!drainOnreadTail(this, true)) {
        socket?.resume?.();
        restorePausedHold(this, socket);
      }
    };
    Socket.prototype._reset = function _reset() {
      debug("Socket.prototype._reset");
      this.resetAndClosing = true;
      return this.destroy();
    };
    Socket.prototype._getpeername = function() {
      if (!this._handle || this.connecting) {
        return this._peername || {};
      } else if (!this._peername) {
        const family = this._handle.remoteFamily;
        if (!family)
          return {};
        this._peername = {
          family,
          address: this._handle.remoteAddress,
          port: this._handle.remotePort
        };
      }
      return this._peername;
    };
    Socket.prototype._getsockname = function() {
      if (!this._handle || this.connecting) {
        return this._sockname || {};
      } else if (!this._sockname) {
        const family = this._handle.localFamily;
        if (!family)
          return {};
        this._sockname = {
          family,
          address: this._handle.localAddress,
          port: this._handle.localPort
        };
      }
      return this._sockname;
    };
    Object.defineProperty(Socket.prototype, "readyState", {
      get: function() {
        if (this.connecting)
          return "opening";
        if (this.readable && this.writable)
          return "open";
        if (this.readable && !this.writable)
          return "readOnly";
        if (!this.readable && this.writable)
          return "writeOnly";
        return "closed";
      }
    });
    Socket.prototype.ref = function ref() {
      this[kUserUnrefed] = false;
      this[kPausedUnref] = false;
      const socket = this._handle;
      if (!socket) {
        this.once("connect", this.ref);
        return this;
      }
      socket.ref();
      return this;
    };
    Object.defineProperty(Socket.prototype, "remotePort", {
      get: function() {
        return this._getpeername().port;
      }
    });
    Object.defineProperty(Socket.prototype, "remoteAddress", {
      get: function() {
        return this._getpeername().address;
      }
    });
    Object.defineProperty(Socket.prototype, "remoteFamily", {
      get: function() {
        return this._getpeername().family;
      }
    });
    function fdSyncWrite(chunk, encoding, callback) {
      const fs = require("node:fs");
      try {
        const buf = typeof chunk === "string" ? Buffer.from(chunk, encoding) : chunk;
        let offset = 0;
        while (offset < buf.length) {
          offset += fs.writeSync(this[kSyncWriteFd], buf, offset);
        }
        this[kBytesWritten] = (this[kBytesWritten] || 0) + offset;
        callback();
      } catch (err) {
        callback(err);
      }
    }
    function fdSyncWritev(data, callback) {
      const fs = require("node:fs");
      try {
        let total = 0;
        for (let i = 0;i < data.length; i++) {
          const { chunk, encoding } = data[i];
          const buf = typeof chunk === "string" ? Buffer.from(chunk, encoding) : chunk;
          let offset = 0;
          while (offset < buf.length) {
            offset += fs.writeSync(this[kSyncWriteFd], buf, offset);
          }
          total += offset;
        }
        this[kBytesWritten] = (this[kBytesWritten] || 0) + total;
        callback();
      } catch (err) {
        callback(err);
      }
    }
    Socket.prototype.resetAndDestroy = function resetAndDestroy() {
      if (this._handle) {
        if (this.connecting) {
          this.once("connect", () => this._reset());
        } else {
          this._reset();
        }
      } else {
        this.destroy(codes.SOCKET_CLOSED());
      }
      return this;
    };
    Socket.prototype.setKeepAlive = function setKeepAlive(enable = false, initialDelayMsecs = 0) {
      enable = Boolean(enable);
      const initialDelay = MathMax(0, ~~initialDelayMsecs);
      if (!this._handle) {
        this[kSetKeepAlive] = enable;
        this[kSetKeepAliveInitialDelay] = initialDelay;
        return this;
      }
      if (!this._handle.setKeepAlive) {
        return this;
      }
      if (enable !== this[kSetKeepAlive] || enable && this[kSetKeepAliveInitialDelay] !== initialDelay) {
        this[kSetKeepAlive] = enable;
        this[kSetKeepAliveInitialDelay] = initialDelay;
        this._handle.setKeepAlive(enable, initialDelay);
      }
      return this;
    };
    Socket.prototype.setNoDelay = function setNoDelay(enable = true) {
      enable = Boolean(enable === undefined ? true : enable);
      if (!this._handle) {
        this[kSetNoDelay] = enable;
        return this;
      }
      if (this._handle.setNoDelay && enable !== this[kSetNoDelay]) {
        this[kSetNoDelay] = enable;
        this._handle.setNoDelay(enable);
      }
      return this;
    };
    Socket.prototype.setTypeOfService = function setTypeOfService(tos) {
      if (Number.isNaN(tos)) {
        throw codes.INVALID_ARG_TYPE("tos", "number", tos);
      }
      validateInt32(tos, "tos", 0, 255);
      if (!this._handle || !this._handle.setTypeOfService) {
        this[kSetTOS] = tos;
        return this;
      }
      if (tos !== this[kSetTOS]) {
        this[kSetTOS] = tos;
        const err = this._handle.setTypeOfService(tos);
        if (err && process.platform !== "win32") {
          throw new ErrnoException(err, "setTypeOfService");
        }
      }
      return this;
    };
    Socket.prototype.getTypeOfService = function getTypeOfService() {
      if (!this._handle || !this._handle.getTypeOfService) {
        return this[kSetTOS] !== undefined ? this[kSetTOS] : 0;
      }
      const res = this._handle.getTypeOfService();
      if (typeof res === "number" && res < 0) {
        if (process.platform === "win32") {
          return this[kSetTOS] !== undefined ? this[kSetTOS] : 0;
        }
        throw new ErrnoException(res, "getTypeOfService");
      }
      return res;
    };
    Socket.prototype.setTimeout = {
      setTimeout(msecs, callback) {
        if (this.destroyed)
          return this;
        this.timeout = msecs;
        msecs = getTimerDuration(msecs, "msecs");
        clearTimeout(this[kTimeout]);
        if (msecs === 0) {
          if (callback !== undefined) {
            validateFunction(callback, "callback");
            this.removeListener("timeout", callback);
          }
        } else {
          this[kTimeout] = setTimeout(this._onTimeout.bind(this), msecs).unref();
          if (callback !== undefined) {
            validateFunction(callback, "callback");
            this.once("timeout", callback);
          }
        }
        return this;
      }
    }.setTimeout;
    Socket.prototype._unrefTimer = function _unrefTimer() {
      for (let s = this;s !== null; s = s._parent) {
        if (s[kTimeout])
          s[kTimeout].refresh();
      }
    };
    Socket.prototype.unref = function unref() {
      this[kUserUnrefed] = true;
      const socket = this._handle;
      if (!socket) {
        this.once("connect", this.unref);
        return this;
      }
      socket.unref();
      return this;
    };
    Socket.prototype.destroySoon = function destroySoon() {
      if (this.writable)
        this.end();
      if (this.writableFinished)
        this.destroy();
      else
        this.once("finish", this.destroy);
    };
    Socket.prototype._writev = function _writev(data, callback) {
      const allBuffers = data.allBuffers;
      const chunks = data;
      if (allBuffers) {
        if (data.length === 1) {
          return this._write(data[0], "buffer", callback);
        }
        for (let i = 0;i < data.length; i++) {
          data[i] = data[i].chunk;
        }
      } else {
        if (data.length === 1) {
          const { chunk, encoding } = data[0];
          return this._write(chunk, encoding, callback);
        }
        for (let i = 0;i < data.length; i++) {
          const { chunk, encoding } = data[i];
          if (typeof chunk === "string") {
            data[i] = Buffer.from(chunk, encoding);
          } else {
            data[i] = chunk;
          }
        }
      }
      const chunk = Buffer.concat(chunks || []);
      return this._write(chunk, "buffer", callback);
    };
    Socket.prototype._write = function _write(chunk, encoding, callback) {
      debug("Socket.prototype._write");
      if (this.connecting) {
        let onClose = function() {
          callback(codes.SOCKET_CLOSED_BEFORE_CONNECTION());
        };
        this[kwriteCallback] = callback;
        this._pendingData = chunk;
        this._pendingEncoding = encoding;
        this.once("connect", function connect() {
          this.off("close", onClose);
          this._write(chunk, encoding, callback);
        });
        this.once("close", onClose);
        return;
      }
      const socket = this._handle;
      if (!socket && this[kupgraded] && !this.destroyed) {
        this[kwriteCallback] = callback;
        this._pendingData = chunk;
        this._pendingEncoding = encoding;
        const onClose = onUpgradeWriteClose.bind(this, callback);
        this.once(kUpgradeAttached, onUpgradeAttachedWrite.bind(this, chunk, encoding, callback, onClose));
        this.once("close", onClose);
        return;
      }
      this._pendingData = null;
      this._pendingEncoding = "";
      this[kwriteCallback] = null;
      if (!socket) {
        callback(codes.SOCKET_CLOSED());
        return false;
      }
      this._unrefTimer();
      if (socket.readyState < 0) {
        const er = new ErrnoException(process.platform === "win32" ? -4047 : -9, "write");
        process.nextTick(callback, er);
        return false;
      }
      const res = socketInternals.write(socket, chunk, encoding);
      this[kBytesWritten] = socket.bytesWritten;
      if (res < 0) {
        process.nextTick(failWrite, this, res, callback);
        return false;
      }
      if (res) {
        if (this.encrypted) {
          process.nextTick(callback);
        } else {
          callback();
        }
      } else if (this[kwriteCallback]) {
        callback(new Error("overlapping _write()"));
      } else {
        this[kwriteCallback] = callback;
        if ((this[kended] || this[kPausedUnref]) && !this[kUserUnrefed])
          socket.ref?.();
      }
    };
    function createConnection(...args) {
      const normalized = normalizeArgs(args);
      const options = normalized[0];
      const socket = new Socket(options);
      const optionsTimeout = options.timeout;
      if (optionsTimeout) {
        socket.setTimeout(optionsTimeout);
      }
      return socket.connect(normalized);
    }
    function lookupAndConnect(self, options) {
      const { localAddress, localPort } = options;
      const host = options.host || "localhost";
      let { port, autoSelectFamilyAttemptTimeout, autoSelectFamily } = options;
      validateString(host, "options.host");
      if (localAddress && !isIP(localAddress)) {
        throw codes.INVALID_IP_ADDRESS(localAddress);
      }
      if (localPort) {
        validateNumber(localPort, "options.localPort");
      }
      if (typeof port !== "undefined") {
        if (typeof port !== "number" && typeof port !== "string") {
          throw codes.INVALID_ARG_TYPE("options.port", ["number", "string"], port);
        }
        validatePort(port);
      }
      port |= 0;
      if (autoSelectFamily != null) {
        validateBoolean(autoSelectFamily, "options.autoSelectFamily");
      } else {
        autoSelectFamily = getDefaultAutoSelectFamily();
      }
      if (autoSelectFamilyAttemptTimeout != null) {
        validateInt32(autoSelectFamilyAttemptTimeout, "options.autoSelectFamilyAttemptTimeout", 1);
        if (autoSelectFamilyAttemptTimeout < 10) {
          autoSelectFamilyAttemptTimeout = 10;
        }
      } else {
        autoSelectFamilyAttemptTimeout = getDefaultAutoSelectFamilyAttemptTimeout();
      }
      self._host = host;
      self._port = port;
      const addressType = isIP(host);
      if (addressType) {
        process.nextTick(() => {
          if (self.connecting) {
            internalConnect(self, options, host, port, addressType, localAddress, localPort);
          }
        });
        return;
      }
      const optionsLookup = options.lookup;
      if (optionsLookup != null)
        validateFunction(optionsLookup, "options.lookup");
      if (dns === undefined)
        dns = require("node:dns");
      const dnsopts = {
        family: socketToDnsFamily(options.family),
        hints: options.hints || 0
      };
      if (!isWindows && dnsopts.family !== 4 && dnsopts.family !== 6 && dnsopts.hints === 0) {
        dnsopts.hints = dns.ADDRCONFIG;
      }
      debug("connect: find host", host, addressType);
      debug("connect: dns options", dnsopts);
      const lookup = optionsLookup || dns.lookup;
      if (dnsopts.family !== 4 && dnsopts.family !== 6 && !localAddress && autoSelectFamily) {
        debug("connect: autodetecting", host, port);
        dnsopts.all = true;
        lookupAndConnectMultiple(self, lookup, host, options, dnsopts, port, localAddress, localPort, autoSelectFamilyAttemptTimeout);
        return;
      }
      lookup(host, dnsopts, function emitLookup(err, ip, addressType) {
        self.emit("lookup", err, ip, addressType, host);
        if (!self.connecting)
          return;
        if (err) {
          process.nextTick(destroyNT, self, err);
        } else if (typeof ip !== "string" || !isIP(ip)) {
          err = codes.INVALID_IP_ADDRESS(ip);
          process.nextTick(destroyNT, self, err);
        } else if (addressType !== 4 && addressType !== 6) {
          err = codes.INVALID_ADDRESS_FAMILY(addressType, options.host, options.port);
          process.nextTick(destroyNT, self, err);
        } else {
          self._unrefTimer();
          internalConnect(self, options, ip, port, addressType, localAddress, localPort);
        }
      });
    }
    function socketToDnsFamily(family) {
      switch (family) {
        case "IPv4":
          return 4;
        case "IPv6":
          return 6;
      }
      return family;
    }
    function lookupAndConnectMultiple(self, lookup, host, options, dnsopts, port, localAddress, localPort, timeout) {
      lookup(host, dnsopts, function emitLookup(err, addresses) {
        if (!self.connecting) {
          return;
        } else if (err) {
          self.emit("lookup", err, undefined, undefined, host);
          process.nextTick(destroyNT, self, err);
          return;
        }
        const validAddresses = [[], []];
        const validIps = [[], []];
        let destinations;
        for (let i = 0, l = addresses.length;i < l; i++) {
          const address = addresses[i];
          const { address: ip, family: addressType } = address;
          self.emit("lookup", err, ip, addressType, host);
          if (!self.connecting) {
            return;
          }
          if (isIP(ip) && (addressType === 4 || addressType === 6)) {
            destinations ||= addressType === 6 ? { 6: 0, 4: 1 } : { 4: 0, 6: 1 };
            const destination = destinations[addressType];
            if (!ArrayPrototypeIncludes.call(validIps[destination], ip)) {
              ArrayPrototypePush.call(validAddresses[destination], address);
              ArrayPrototypePush.call(validIps[destination], ip);
            }
          }
        }
        if (!validAddresses[0].length && !validAddresses[1].length) {
          const { address: firstIp, family: firstAddressType } = addresses[0];
          if (!isIP(firstIp)) {
            err = codes.INVALID_IP_ADDRESS(firstIp);
            process.nextTick(destroyNT, self, err);
          } else if (firstAddressType !== 4 && firstAddressType !== 6) {
            err = codes.INVALID_ADDRESS_FAMILY(firstAddressType, options.host, options.port);
            process.nextTick(destroyNT, self, err);
          }
          return;
        }
        const toAttempt = [];
        for (let i = 0, l = MathMax(validAddresses[0].length, validAddresses[1].length);i < l; i++) {
          if (i in validAddresses[0]) {
            ArrayPrototypePush.call(toAttempt, validAddresses[0][i]);
          }
          if (i in validAddresses[1]) {
            ArrayPrototypePush.call(toAttempt, validAddresses[1][i]);
          }
        }
        if (toAttempt.length === 1) {
          debug("connect/multiple: only one address found, switching back to single connection");
          const { address: ip, family: addressType } = toAttempt[0];
          self._unrefTimer();
          internalConnect(self, options, ip, port, addressType, localAddress, localPort);
          return;
        }
        self.autoSelectFamilyAttemptedAddresses = [];
        debug("connect/multiple: will try the following addresses", toAttempt);
        const context = {
          socket: self,
          addresses: toAttempt,
          current: 0,
          port,
          localPort,
          timeout,
          [kTimeout]: null,
          errors: [],
          options
        };
        self._unrefTimer();
        internalConnectMultiple(context);
      });
    }
    function internalConnect(self, options, address, port, addressType, localAddress, localPort, _flags) {
      assert(self.connecting);
      let err;
      if (localAddress || localPort) {
        if (addressType === 4) {
          localAddress ||= "0.0.0.0";
        } else {
          localAddress ||= "::";
        }
        debug("connect: binding to localAddress: %s and localPort: %d (addressType: %d)", localAddress, localPort, addressType);
        err = checkBindError(err, localPort, self._handle);
        if (err) {
          const ex = new ExceptionWithHostPort(err, "bind", localAddress, localPort);
          self.destroy(ex);
          return;
        }
      }
      let connection = self[ksocket];
      const optionsSocket = options.socket;
      if (optionsSocket) {
        connection = optionsSocket;
      }
      let tls = undefined;
      const bunTLS = self[bunTlsSymbol];
      if (typeof bunTLS === "function") {
        tls = bunTLS.call(self, port, self._host, true);
        self._requestCert = true;
        if (tls) {
          const { rejectUnauthorized, session, checkServerIdentity } = options;
          applyRejectUnauthorized(self, tls, rejectUnauthorized);
          tls.requestCert = true;
          tls.session = session || tls.session;
          self.servername = tls.servername;
          tls.checkServerIdentity = checkServerIdentity || tls.checkServerIdentity;
          self[bunTLSConnectOptions] = tls;
          let tlsSocket;
          if (!connection && (tlsSocket = tls.socket)) {
            connection = tlsSocket;
          }
        }
        self.authorized = false;
        self.secureConnecting = true;
        self._secureEstablished = false;
        self._securePending = true;
        self[kConnectOptions] = options;
        self.prependListener("end", onConnectEnd);
      }
      debug("connect: attempting to connect to %s:%d (addressType: %d)", address, port, addressType);
      self.emit("connectionAttempt", address, port, addressType);
      if (addressType === 6 || addressType === 4) {
        if (self.blockList?.check(address, `ipv${addressType}`)) {
          self.destroy(codes.IP_BLOCKED(address));
          return;
        }
        const req = {};
        req.oncomplete = afterConnect;
        req.address = address;
        req.port = port;
        req.localAddress = localAddress;
        req.localPort = localPort;
        req.addressType = addressType;
        req.tls = tls;
        req.pauseOnConnect = options.pauseOnConnect;
        traceConnectStart(req);
        err = kConnectTcp(self, addressType, req, address, port);
        if (!err && hasObserver("net")) {
          startPerf(self, kPerfHooksNetConnectContext, {
            type: "net",
            name: "connect",
            detail: { host: address, port }
          });
        }
      } else {
        const req = {};
        req.address = address;
        req.oncomplete = afterConnect;
        req.tls = tls;
        req.pauseOnConnect = options.pauseOnConnect;
        traceConnectStart(req, address);
        err = kConnectPipe(self, req, address);
      }
      if (err) {
        const ex = new ExceptionWithHostPort(err, "connect", address, port);
        self.destroy(ex);
      }
    }
    function internalConnectMultiple(context, canceled) {
      clearTimeout(context[kTimeout]);
      const self = context.socket;
      if (self._aborted) {
        return;
      }
      if (canceled || context.current === context.addresses.length) {
        if (context.errors.length === 0) {
          self.destroy(codes.SOCKET_CONNECTION_TIMEOUT());
          return;
        }
        self.destroy(new NodeAggregateError(context.errors));
        return;
      }
      assert(self.connecting);
      const current = context.current++;
      if (current > 0) {
        self[kReinitializeHandle](newDetachedSocket(typeof self[bunTlsSymbol] === "function"));
      }
      const { localPort, port, _flags } = context;
      const { address, family: addressType } = context.addresses[current];
      let localAddress;
      let err;
      if (localPort) {
        if (addressType === 4) {
          localAddress = "0.0.0.0";
        } else {
          localAddress = "::";
        }
        debug("connect/multiple: binding to localAddress: %s and localPort: %d (addressType: %d)", localAddress, localPort, addressType);
        err = checkBindError(err, localPort, self._handle);
        if (err) {
          ArrayPrototypePush.call(context.errors, new ExceptionWithHostPort(err, "bind", localAddress, localPort));
          internalConnectMultiple(context);
          return;
        }
      }
      if (self.blockList?.check(address, `ipv${addressType}`)) {
        const ex = codes.IP_BLOCKED(address);
        ArrayPrototypePush.call(context.errors, ex);
        self.emit("connectionAttemptFailed", address, port, addressType, ex);
        internalConnectMultiple(context);
        return;
      }
      let connection = self[ksocket];
      const contextOptionsSocket = context.options.socket;
      if (contextOptionsSocket) {
        connection = contextOptionsSocket;
      }
      let tls = undefined;
      const bunTLS = self[bunTlsSymbol];
      if (typeof bunTLS === "function") {
        tls = bunTLS.call(self, port, self._host, true);
        self._requestCert = true;
        if (tls) {
          const { rejectUnauthorized, session, checkServerIdentity } = context.options;
          applyRejectUnauthorized(self, tls, rejectUnauthorized);
          tls.requestCert = true;
          tls.session = session || tls.session;
          self.servername = tls.servername;
          tls.checkServerIdentity = checkServerIdentity || tls.checkServerIdentity;
          self[bunTLSConnectOptions] = tls;
          let tlsSocket;
          if (!connection && (tlsSocket = tls.socket)) {
            connection = tlsSocket;
          }
        }
        self.authorized = false;
        self.secureConnecting = true;
        self._secureEstablished = false;
        self._securePending = true;
        self[kConnectOptions] = context.options;
        self.prependListener("end", onConnectEnd);
      }
      debug("connect/multiple: attempting to connect to %s:%d (addressType: %d)", address, port, addressType);
      self.emit("connectionAttempt", address, port, addressType);
      const req = {};
      req.oncomplete = afterConnectMultiple.bind(undefined, context, current);
      req.address = address;
      req.port = port;
      req.localAddress = localAddress;
      req.localPort = localPort;
      req.addressType = addressType;
      req.tls = tls;
      req.pauseOnConnect = context.options.pauseOnConnect;
      ArrayPrototypePush.call(self.autoSelectFamilyAttemptedAddresses, `${address}:${port}`);
      traceConnectStart(req);
      err = kConnectTcp(self, addressType, req, address, port);
      if (err) {
        const ex = new ExceptionWithHostPort(err, "connect", address, port);
        ArrayPrototypePush.call(context.errors, ex);
        self.emit("connectionAttemptFailed", address, port, addressType, ex);
        if (self.connecting)
          internalConnectMultiple(context);
        return;
      }
      if (!self.connecting || context.current !== current + 1) {
        return;
      }
      if (hasObserver("net")) {
        startPerf(context, kPerfHooksNetConnectContext, {
          type: "net",
          name: "connect",
          detail: { host: address, port }
        });
      }
      if (current < context.addresses.length - 1) {
        debug("connect/multiple: setting the attempt timeout to %d ms", context.timeout);
        context[kTimeout] = setTimeout(internalConnectMultipleTimeout, context.timeout, context, req, self._handle).unref();
      }
    }
    function internalConnectMultipleTimeout(context, req, handle) {
      if (!context.socket.connecting)
        return;
      debug("connect/multiple: connection to %s:%s timed out", req.address, req.port);
      context.socket.emit("connectionAttemptTimeout", req.address, req.port, req.addressType);
      req.oncomplete = undefined;
      traceConnectEnd(req);
      ArrayPrototypePush.call(context.errors, createConnectionError(req, uv().UV_ETIMEDOUT));
      handle.close();
      if (context.socket.connecting) {
        internalConnectMultiple(context);
      }
    }
    function afterConnect(status, handle, req, readable, writable) {
      traceConnectEnd(req);
      if (!handle)
        return;
      const self = handle[owner_symbol];
      if (!self)
        return;
      if (self.destroyed) {
        return;
      }
      debug("afterConnect", status, readable, writable);
      if (!self.connecting)
        return;
      self.connecting = false;
      self._sockname = null;
      if (status === 0) {
        if (self.readable && !readable) {
          self.push(null);
          self.read();
        }
        if (self.writable && !writable) {
          self.end();
        }
        self._unrefTimer();
        if (self[kSetTOS] !== undefined && self._handle.setTypeOfService) {
          self._handle.setTypeOfService(self[kSetTOS]);
        }
        if (self[kSetNoDelay] && self._handle.setNoDelay) {
          self._handle.setNoDelay(true);
        }
        if (self[kSetKeepAlive] && self._handle.setKeepAlive) {
          self._handle.setKeepAlive(true, self[kSetKeepAliveInitialDelay]);
        }
        const pausedBeforeConnect = self.isPaused();
        if (pausedBeforeConnect && !self.encrypted)
          readStop(self, self._handle);
        self.emit("connect");
        self.emit("ready");
        if (self[kPerfHooksNetConnectContext] && hasObserver("net")) {
          stopPerf(self, kPerfHooksNetConnectContext);
        }
        if (readable && !self.isPaused())
          self.read(0);
        else if (!pausedBeforeConnect && !self.encrypted && !self._readableState.reading)
          readStop(self, self._handle);
      } else {
        let details;
        const localAddress = req.localAddress;
        let localPort;
        if (localAddress && (localPort = req.localPort)) {
          details = localAddress + ":" + localPort;
        }
        const ex = new ExceptionWithHostPort(status, "connect", req.address, req.port, details);
        if (details) {
          ex.localAddress = req.localAddress;
          ex.localPort = req.localPort;
        }
        self.emit("connectionAttemptFailed", req.address, req.port, req.addressType, ex);
        self.destroy(ex);
      }
    }
    function afterConnectMultiple(context, current, status, handle, req, readable, writable) {
      traceConnectEnd(req);
      debug("connect/multiple: connection attempt to %s:%s completed with status %s", req.address, req.port, status);
      debug("clearTimeout", context[kTimeout]);
      clearTimeout(context[kTimeout]);
      if (status === 0 && current !== context.current - 1) {
        debug("connect/multiple: ignoring successful but timedout connection to %s:%s", req.address, req.port);
        handle.close();
        return;
      }
      const self = context.socket;
      if (status !== 0) {
        const ex = createConnectionError(req, status);
        ArrayPrototypePush.call(context.errors, ex);
        self.emit("connectionAttemptFailed", req.address, req.port, req.addressType, ex);
        if (context.socket.connecting) {
          internalConnectMultiple(context, status === uv().UV_ECANCELED);
        }
        return;
      }
      if (hasObserver("net") && context[kPerfHooksNetConnectContext]) {
        self[kPerfHooksNetConnectContext] = context[kPerfHooksNetConnectContext];
      }
      afterConnect(status, self._handle, req, readable, writable);
    }
    function createConnectionError(req, status) {
      let details;
      const localAddress = req.localAddress;
      let localPort;
      if (localAddress && (localPort = req.localPort)) {
        details = localAddress + ":" + localPort;
      }
      const ex = new ExceptionWithHostPort(status, "connect", req.address, req.port, details);
      if (details) {
        ex.localAddress = req.localAddress;
        ex.localPort = req.localPort;
      }
      return ex;
    }
    function Server(options, connectionListener) {
      if (!(this instanceof Server)) {
        return new Server(options, connectionListener);
      }
      EventEmitter.apply(this, []);
      if (typeof options === "function") {
        connectionListener = options;
        options = {};
      } else if (options == null || typeof options === "object") {
        options = { ...options };
      } else {
        throw codes.INVALID_ARG_TYPE("options", ["Object", "Function"], options);
      }
      let {
        allowHalfOpen = false,
        keepAlive = false,
        keepAliveInitialDelay,
        highWaterMark = getDefaultHighWaterMark(),
        pauseOnConnect = false,
        noDelay = false
      } = options;
      if (keepAliveInitialDelay !== undefined) {
        validateNumber(keepAliveInitialDelay, "options.keepAliveInitialDelay");
        if (keepAliveInitialDelay < 0)
          keepAliveInitialDelay = 0;
      } else {
        keepAliveInitialDelay = 0;
      }
      this._connections = 0;
      this._handle = null;
      this._usingWorkers = false;
      this.workers = [];
      this._unref = false;
      this[bunSocketServerOptions] = undefined;
      this.allowHalfOpen = allowHalfOpen;
      this.keepAlive = Boolean(keepAlive);
      this.keepAliveInitialDelay = MathMax(0, ~~keepAliveInitialDelay);
      this.highWaterMark = highWaterMark;
      this.pauseOnConnect = Boolean(pauseOnConnect);
      this.noDelay = Boolean(noDelay);
      if (typeof connectionListener === "function") {
        this.on("connection", connectionListener);
      }
      this[bunSocketServerOptions] = options;
      const optionsBlockList = options.blockList;
      if (optionsBlockList) {
        if (!lazyBlockList().isBlockList(optionsBlockList)) {
          throw codes.INVALID_ARG_TYPE("options.blockList", "net.BlockList", optionsBlockList);
        }
        this.blockList = optionsBlockList;
      }
    }
    toClass(Server, "Server", EventEmitter);
    Object.defineProperty(Server.prototype, "listening", {
      get() {
        return !!this._handle;
      }
    });
    Server.prototype.ref = function ref() {
      this._unref = false;
      this._handle?.ref();
      return this;
    };
    Server.prototype.unref = function unref() {
      this._unref = true;
      this._handle?.unref();
      return this;
    };
    Server.prototype.close = function close(callback) {
      this[kClusterListeningId] = (this[kClusterListeningId] || 0) + 1;
      if (typeof callback === "function") {
        if (!this._handle) {
          this.once("close", function close() {
            callback(codes.SERVER_NOT_RUNNING());
          });
        } else {
          this.once("close", callback);
        }
      }
      if (this._handle) {
        if (typeof this._handle.stop === "function") {
          this._handle.stop(false);
          setImmediate(noop);
          const clusterHandle = this[kClusterHandle];
          if (clusterHandle) {
            this[kClusterHandle] = null;
            this[kClusterUnixPath] = undefined;
            clusterHandle.close();
          }
        } else {
          this._handle.close();
        }
        this._handle = null;
      }
      this._emitCloseIfDrained();
      return this;
    };
    Server.prototype[Symbol.asyncDispose] = function() {
      if (!this._handle)
        return Promise.resolve();
      const { resolve, reject, promise } = Promise.withResolvers();
      this.close(function(err, ...args) {
        if (err)
          reject(err);
        else
          resolve(...args);
      });
      return promise;
    };
    Server.prototype._emitCloseIfDrained = function _emitCloseIfDrained() {
      if (this._handle || this._connections > 0) {
        return;
      }
      process.nextTick(() => {
        this.emit("close");
      });
    };
    Server.prototype.address = function address() {
      const server = this._handle;
      if (server) {
        const unix = server.unix || this[kClusterUnixPath];
        if (unix) {
          return unix;
        }
        const out = {};
        const err = this._handle.getsockname(out);
        if (err)
          throw new ErrnoException(err, "address");
        return out;
      }
      return null;
    };
    Server.prototype.getConnections = function getConnections(callback) {
      if (typeof callback === "function") {
        callback(null, this._handle ? this._connections : 0);
      }
      return this;
    };
    Server.prototype.listen = function listen(port, hostname, onListen) {
      const argsLength = arguments.length;
      if (typeof port === "string") {
        const numPort = Number(port);
        if (!Number.isNaN(numPort))
          port = numPort;
      }
      let backlog;
      let path;
      let exclusive = false;
      let reusePort = false;
      let ipv6Only = false;
      let readableAll = false;
      let writableAll = false;
      let fd;
      if (typeof port === "string") {
        if (Number.isSafeInteger(hostname)) {
          if (hostname > 0) {
            backlog = hostname;
          }
        } else if (typeof hostname === "function") {
          onListen = hostname;
        }
        path = port;
        hostname = undefined;
        port = undefined;
      } else {
        if (typeof hostname === "number") {
          backlog = hostname;
          hostname = undefined;
        } else if (typeof hostname === "function") {
          onListen = hostname;
          hostname = undefined;
        } else if (typeof hostname === "string" && typeof onListen === "number") {
          backlog = onListen;
          onListen = argsLength > 3 ? arguments[3] : undefined;
        }
        if (typeof port === "function") {
          onListen = port;
          port = 0;
        } else if (port !== null && typeof port === "object") {
          const options = port;
          addServerAbortSignalOption(this, options);
          hostname = options.host;
          exclusive = options.exclusive;
          path = options.path;
          port = options.port;
          ipv6Only = options.ipv6Only;
          reusePort = options.reusePort;
          backlog = options.backlog;
          readableAll = options.readableAll;
          writableAll = options.writableAll;
          const optionsFd = options.fd;
          if (typeof optionsFd === "number" && optionsFd >= 0) {
            fd = optionsFd;
            port = 0;
          }
          const isLinux = process.platform === "linux" || process.platform === "android";
          if (port === undefined && "port" in options || port === null) {
            port = 0;
          }
          if (typeof port === "number" || typeof port === "string") {
            validatePort(port, "options.port");
            port = port | 0;
            path = undefined;
          } else if (isPipeName(path)) {
            const isAbstractPath = path.startsWith("\x00");
            if (isLinux && isAbstractPath && (options.writableAll || options.readableAll)) {
              const message = `The argument 'options' can not set readableAll or writableAll to true when path is abstract unix socket. Received ${JSON.stringify(options)}`;
              const error = new TypeError(message);
              error.code = "ERR_INVALID_ARG_VALUE";
              throw error;
            }
            hostname = path;
            port = undefined;
          } else if (!("port" in options) && !("path" in options)) {
            let message = `The argument 'options' must have the property "port" or "path"`;
            try {
              message = `${message}. Received ${JSON.stringify(options)}`;
            } catch {}
            const error = new TypeError(message);
            error.code = "ERR_INVALID_ARG_VALUE";
            throw error;
          } else {
            let message = "The argument 'options' is invalid";
            try {
              message = `${message}. Received ${JSON.stringify(options)}`;
            } catch {}
            const error = new TypeError(message);
            error.code = "ERR_INVALID_ARG_VALUE";
            throw error;
          }
          if (typeof options.callback === "function")
            onListen = options?.callback;
        } else if (port === undefined || port === null) {
          port = 0;
        } else if (typeof port === "number" || typeof port === "string") {
          validatePort(port, "options.port");
          port = port | 0;
        } else {
          let message = "The argument 'options' is invalid";
          try {
            message = `${message}. Received ${JSON.stringify(port)}`;
          } catch {}
          const error = new TypeError(message);
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (reusePort === true) {
          exclusive = true;
        }
        var clusterHost = typeof hostname === "string" && hostname.length > 0 ? hostname : null;
        hostname = hostname || "::";
      }
      if (typeof port === "number" && (port < 0 || port >= 65536)) {
        throw codes.SOCKET_BAD_PORT(`options.port should be >= 0 and < 65536. Received type number: (${port})`);
      }
      if (this._handle) {
        throw codes.SERVER_ALREADY_LISTEN();
      }
      if (onListen != null) {
        this.once("listening", onListen);
      }
      try {
        var tls = undefined;
        var TLSSocketClass = undefined;
        const bunTLS = this[bunTlsSymbol];
        const options = this[bunSocketServerOptions];
        let contexts = null;
        if (typeof bunTLS === "function") {
          [tls, TLSSocketClass] = bunTLS.call(this, port, hostname, false);
          options.servername = tls.serverName;
          options[kSocketClass] = TLSSocketClass;
          contexts = tls.contexts;
          if (!tls.requestCert) {
            tls.rejectUnauthorized = false;
          }
        } else {
          options[kSocketClass] = Socket;
        }
        const flags = (ipv6Only === true ? 1 : 0) | (reusePort === true ? 2 : 0);
        let queryAddress = null;
        let queryPort = port;
        let queryAddressType = 4;
        if (path) {
          queryAddress = path;
          queryPort = -1;
          queryAddressType = -1;
        } else if (typeof fd === "number" && fd >= 0) {
          queryPort = null;
          queryAddressType = null;
        } else if (typeof clusterHost === "string") {
          queryAddress = clusterHost;
          queryAddressType = isIP(clusterHost) || 4;
        }
        listenInCluster(this, queryAddress, queryPort, queryAddressType, backlog, fd, exclusive, ipv6Only, reusePort, readableAll, writableAll, flags, undefined, path, hostname, tls, contexts, onListen);
      } catch (err) {
        const isUnix = path != null;
        process.nextTick(emitErrorNextTick, this, formatListenError(err, isUnix ? path : hostname, isUnix ? undefined : port));
      }
      return this;
    };
    Server.prototype[kRealListen] = function(path, port, hostname, exclusive, ipv6Only, reusePort, readableAll, writableAll, tls, contexts, _onListen, fd) {
      if (reusePort) {
        exclusive = false;
      }
      if (path) {
        this._handle = Bun.listen({
          unix: path,
          tls,
          allowHalfOpen: true,
          reusePort: reusePort || this[bunSocketServerOptions]?.reusePort || false,
          ipv6Only: ipv6Only || this[bunSocketServerOptions]?.ipv6Only || false,
          exclusive: exclusive || this[bunSocketServerOptions]?.exclusive || false,
          socket: serverHandlersFor(this),
          data: this,
          pauseOnConnect: this.pauseOnConnect
        });
        if ((readableAll || writableAll) && process.platform !== "win32" && path.charCodeAt(0) !== 0) {
          let desired = 0;
          if (readableAll)
            desired |= 36;
          if (writableAll)
            desired |= 18;
          try {
            const fs = require("node:fs");
            const cur = fs.statSync(path).mode;
            if ((cur & desired) !== desired)
              fs.chmodSync(path, cur | desired);
          } catch (e) {
            this._handle?.stop?.(true);
            this._handle = null;
            throw e;
          }
        }
      } else if (fd != null) {
        this._handle = Bun.listen({
          fd,
          hostname,
          tls,
          allowHalfOpen: true,
          reusePort: reusePort || this[bunSocketServerOptions]?.reusePort || false,
          ipv6Only: ipv6Only || this[bunSocketServerOptions]?.ipv6Only || false,
          exclusive: exclusive || this[bunSocketServerOptions]?.exclusive || false,
          socket: serverHandlersFor(this),
          data: this,
          pauseOnConnect: this.pauseOnConnect
        });
      } else {
        this._handle = Bun.listen({
          port,
          hostname,
          tls,
          allowHalfOpen: true,
          reusePort: reusePort || this[bunSocketServerOptions]?.reusePort || false,
          ipv6Only: ipv6Only || this[bunSocketServerOptions]?.ipv6Only || false,
          exclusive: exclusive || this[bunSocketServerOptions]?.exclusive || false,
          socket: serverHandlersFor(this),
          data: this,
          pauseOnConnect: this.pauseOnConnect
        });
      }
      this._handle[owner_symbol] = this;
      this._handle.onconnection = onconnection;
      const addr = this.address();
      if (addr && typeof addr === "object") {
        const familyLast = String(addr.family).slice(-1);
        this._connectionKey = `${familyLast}:${addr.address}:${port}`;
      }
      if (contexts) {
        for (const [name, context] of contexts) {
          addServerName(this._handle, name, context.context ?? context);
        }
      }
      if (this._unref)
        this.unref();
      process.nextTick(emitListeningNextTick, this);
    };
    Server.prototype[EventEmitter.captureRejectionSymbol] = function(err, event, sock) {
      switch (event) {
        case "connection":
        case "secureConnection":
          sock.destroy(err);
          break;
        default:
          this.emit("error", err);
      }
    };
    Server.prototype.getsockname = function getsockname(out) {
      out.port = this.address().port;
      return out;
    };
    function emitErrorNextTick(self, error) {
      self.emit("error", error);
    }
    function emitErrorAndCloseNextTick(self, error) {
      self.emit("error", error);
      self.emit("close", true);
    }
    function addServerAbortSignalOption(self, options) {
      if (options?.signal === undefined) {
        return;
      }
      validateAbortSignal(options.signal, "options.signal");
      const { signal } = options;
      const onAborted = () => self.close();
      if (signal.aborted) {
        process.nextTick(onAborted);
      } else {
        signal.addEventListener("abort", onAborted);
      }
    }
    function emitListeningNextTick(self) {
      if (!self._handle)
        return;
      self.emit("listening");
    }
    const { isPrimary } = require("internal/cluster/isPrimary");
    let cluster;
    function listenInCluster(server, address, port, addressType, backlog, fd, exclusive, ipv6Only, reusePort, readableAll, writableAll, flags, options, path, hostname, tls, contexts, onListen) {
      exclusive = !!exclusive;
      if (!isPrimary && cluster === undefined)
        cluster = require("node:cluster");
      if (!isPrimary && !exclusive && typeof address === "string" && address.length > 0 && typeof port === "number" && port >= 0 && isIP(address) === 0) {
        const lookupListeningId = server[kClusterListeningId] = (server[kClusterListeningId] || 0) + 1;
        require("node:dns").lookup(address, (err, ip, family) => {
          if (lookupListeningId !== server[kClusterListeningId])
            return;
          if (err) {
            server.emit("error", err);
            return;
          }
          listenInCluster(server, ip, port, family === 6 ? 6 : 4, backlog, fd, exclusive, ipv6Only, reusePort, readableAll, writableAll, flags, options, path, hostname, tls, contexts, onListen);
        });
        return;
      }
      if (isPrimary || exclusive) {
        server[kRealListen](path, port, hostname, exclusive, ipv6Only, reusePort, readableAll, writableAll, tls, contexts, onListen, fd);
        return;
      }
      const serverQuery = {
        address,
        port,
        addressType,
        fd,
        flags,
        backlog,
        readableAll,
        writableAll,
        ...options,
        sharedOnly: tls ? true : undefined
      };
      const listeningId = server[kClusterListeningId] = (server[kClusterListeningId] || 0) + 1;
      cluster._getServer(server, serverQuery, function listenOnPrimaryHandle(err, handle, _reply) {
        if (listeningId !== server[kClusterListeningId]) {
          handle?.close();
          return;
        }
        err = checkBindError(err, port, handle);
        if (err) {
          const ex = new ExceptionWithHostPort(err, "bind", address, port);
          if (typeof _reply?.bunHint === "string")
            ex.message += `
      note: ${_reply.bunHint}`;
          server.emit("error", ex);
          return;
        }
        const sharedFd = handle?.sharedFd;
        if (handle && typeof sharedFd === "number") {
          server[kClusterHandle] = handle;
          handle[kClusterOwner] = server;
          server[kClusterUnixPath] = path;
          try {
            server[kRealListen](undefined, port, hostname, exclusive, ipv6Only, reusePort, readableAll, writableAll, tls, contexts, onListen, sharedFd);
            handle.adopted = true;
          } catch (err) {
            server[kClusterHandle] = null;
            server[kClusterUnixPath] = undefined;
            handle[kClusterOwner] = null;
            handle.close();
            process.nextTick(emitErrorNextTick, server, err);
          }
          return;
        }
        server[kClusterFauxListen](handle, backlog, path);
      });
    }
    const kClusterListeningId = Symbol("kClusterListeningId");
    const kClusterHandle = Symbol("kClusterHandle");
    const kClusterUnixPath = Symbol("kClusterUnixPath");
    const kClusterFauxListen = Symbol("kClusterFauxListen");
    const { kClusterOwner } = require("internal/shared");
    Server.prototype[kClusterFauxListen] = function(handle, backlog, path) {
      this[kClusterHandle] = handle;
      this._handle = handle;
      if (path) {
        handle.unix = path;
      }
      handle.onconnection = onClusterConnection;
      handle[kClusterOwner] = this;
      handle.listen(backlog || 511);
      if (this._unref)
        this.unref();
      process.nextTick(emitListeningNextTick, this);
    };
    function onClusterConnection(err, clientHandle) {
      const self = this[kClusterOwner];
      if (!self || self[kClusterHandle] !== this) {
        clientHandle?.close();
        return;
      }
      if (err) {
        self.emit("error", new ErrnoException(err, "accept"));
        return;
      }
      if (self.maxConnections != null && self._connections >= self.maxConnections) {
        self.emit("drop");
        clientHandle.close();
        return;
      }
      const socket = new Socket({
        allowHalfOpen: self.allowHalfOpen,
        highWaterMark: self.highWaterMark
      });
      socket.isServer = true;
      if (self.noDelay)
        socket[kSetNoDelay] = true;
      if (self.keepAlive) {
        socket[kSetKeepAlive] = true;
        socket[kSetKeepAliveInitialDelay] = self.keepAliveInitialDelay;
      }
      socket.connect({ fd: clientHandle.fd, fdIsRawSocket: true, pauseOnConnect: self.pauseOnConnect });
      const blockList = self.blockList;
      if (blockList) {
        const remote = socket.remoteAddress;
        const t = isIP(remote);
        if (t && blockList.check(remote, `ipv${t}`)) {
          socket.destroy();
          return;
        }
      }
      socket.server = self;
      socket._server = self;
      self._connections++;
      self.emit("connection", socket);
    }
    function createServer(options, connectionListener) {
      return new Server(options, connectionListener);
    }
    function normalizeArgs(args) {
      let arr;
      if (args.length === 0) {
        arr = [{}, null];
        arr[normalizedArgsSymbol] = true;
        return arr;
      }
      const arg0 = args[0];
      let options = {};
      if (typeof arg0 === "object" && arg0 !== null) {
        options = arg0;
      } else if (isPipeName(arg0)) {
        options.path = arg0;
      } else {
        options.port = arg0;
        if (args.length > 1 && typeof args[1] === "string") {
          options.host = args[1];
        }
      }
      const cb = args[args.length - 1];
      if (typeof cb !== "function")
        arr = [options, null];
      else
        arr = [options, cb];
      arr[normalizedArgsSymbol] = true;
      return arr;
    }
    function initSocketHandle(self) {
      self._undestroy();
      self._sockname = null;
      self[kclosed] = false;
      self[kended] = false;
      const handle = self._handle;
      if (handle) {
        handle[owner_symbol] = self;
        if (self[kUserUnrefed] || self[kPausedUnref])
          handle.unref?.();
      }
    }
    function onSocketHandleClosed() {}
    function closeSocketHandle(self, isException, isCleanupPending = false) {
      const handle = self._handle;
      debug("closeSocketHandle", isException, isCleanupPending, !!handle);
      if (handle) {
        handle.close(onSocketHandleClosed);
        setImmediate(() => {
          debug("emit close", isCleanupPending);
          self.emit("close", isException);
          if (isCleanupPending) {
            handle.onread = noop;
            if (self._handle === handle) {
              self._handle = null;
              self._sockname = null;
            }
          }
        });
      }
    }
    function uvListenErrorDescription(code) {
      switch (code) {
        case "EADDRINUSE":
          return "address already in use";
        case "EACCES":
          return "permission denied";
        case "EADDRNOTAVAIL":
          return "address not available";
        case "EINVAL":
          return "invalid argument";
        default:
          return;
      }
    }
    function formatListenError(err, address, port) {
      const desc = err && typeof err.code === "string" ? uvListenErrorDescription(err.code) : undefined;
      if (desc) {
        err.syscall = "listen";
        err.address = address;
        if (port)
          err.port = port;
        const where = port ? `${address}:${port}` : address;
        err.message = `listen ${err.code}: ${desc}${where ? ` ${where}` : ""}`;
      }
      return err;
    }
    function checkBindError(err, port, handle) {
      if (err === 0 && port > 0 && handle.getsockname) {
        const out = {};
        err = handle.getsockname(out);
        let outPort;
        if (err === 0 && port !== (outPort = out.port)) {
          debug(`checkBindError, bound to ${outPort} instead of ${port}`);
          const UV_EADDRINUSE = -4091;
          err = UV_EADDRINUSE;
        }
      }
      return err;
    }
    function isPipeName(s) {
      return typeof s === "string" && toNumber(s) === false;
    }
    function toNumber(x) {
      return (x = Number(x)) >= 0 ? x : false;
    }
    let warnSimultaneousAccepts = true;
    function _setSimultaneousAccepts() {
      if (warnSimultaneousAccepts) {
        process.emitWarning("net._setSimultaneousAccepts() is deprecated and will be removed.", "DeprecationWarning", "DEP0121");
        warnSimultaneousAccepts = false;
      }
    }
    Server.prototype[kArmHandshakeTimeout] = function(socket) {
      initAcceptedTLSSocket(this, socket);
    };
    const netExports = {
      createServer,
      Server,
      createConnection,
      connect: createConnection,
      isIP,
      isIPv4,
      isIPv6,
      Socket,
      _normalizeArgs: normalizeArgs,
      _setSimultaneousAccepts,
      getDefaultAutoSelectFamily,
      setDefaultAutoSelectFamily,
      getDefaultAutoSelectFamilyAttemptTimeout,
      setDefaultAutoSelectFamilyAttemptTimeout,
      get BlockList() {
        return lazyBlockList();
      },
      set BlockList(value) {
        Object.defineProperty(netExports, "BlockList", { value, writable: true, enumerable: true, configurable: true });
      },
      get SocketAddress() {
        return SocketAddress ??= notImplemented("net.SocketAddress");
      },
      set SocketAddress(value) {
        Object.defineProperty(netExports, "SocketAddress", { value, writable: true, enumerable: true, configurable: true });
      },
      Stream: Socket
    };
    return netExports;
  })();
})
