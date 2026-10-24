// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/_http2_upgrade.ts` do bun 1.4.2. Não editar à mão.
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
  const require = name => (name.startsWith("internal/") || name.startsWith("native/") ? host.internalModule(name) : host.requireBuiltin(name));
  return (() => {
    const { Duplex } = require("node:stream");
    const upgradeDuplexToTLS = notImplemented("http2 TLS upgrade");
    const kSharedCreds = Symbol.for("::buntlssharedcreds::");
    function UpgradeContext(connectionListener, server, rawSocket) {
      this.connectionListener = connectionListener;
      this.server = server;
      this.rawSocket = rawSocket;
      this.nativeHandle = null;
      this.events = null;
    }
    function tlsSocketRead() {
      const h = this._ctx.nativeHandle;
      if (h) {
        h.resume();
      }
      this._ctx.rawSocket.resume();
    }
    function tlsSocketWrite(chunk, encoding, callback) {
      const h = this._ctx.nativeHandle;
      if (!h) {
        callback(new Error("Socket is closed"));
        return;
      }
      if (h.write(chunk, encoding)) {
        callback();
      } else {
        this._writeCallback = callback;
      }
    }
    function tlsSocketDestroy(err, callback) {
      const h = this._ctx.nativeHandle;
      if (h) {
        h.close();
        this._ctx.nativeHandle = null;
      }
      const writeCb = this._writeCallback;
      if (writeCb) {
        this._writeCallback = null;
        writeCb(err ?? new Error("Socket destroyed"));
      }
      callback(err);
    }
    function tlsSocketFinal(callback) {
      const h = this._ctx.nativeHandle;
      if (!h)
        return callback();
      h.end();
      callback();
    }
    function socketOpen() {}
    function socketData(_socket, chunk) {
      if (!this.push(chunk)) {
        this._ctx.rawSocket.pause();
      }
    }
    function socketEnd() {
      this.push(null);
    }
    function socketDrain() {
      const cb = this._writeCallback;
      if (cb) {
        this._writeCallback = null;
        cb();
      }
    }
    function socketClose() {
      if (!this.destroyed) {
        this.destroy();
      }
    }
    function socketError(_socket, err) {
      const ctx = this._ctx;
      if (!ctx.server._requestCert && err?.code === "UNABLE_TO_GET_ISSUER_CERT") {
        return;
      }
      this.destroy(err);
    }
    function socketTimeout() {
      this.emit("timeout");
    }
    function socketHandshake(nativeHandle, success, verifyError) {
      const tlsSocket = this;
      const ctx = tlsSocket._ctx;
      if (!success) {
        const err = verifyError || new Error("TLS handshake failed");
        ctx.server.emit("tlsClientError", err, tlsSocket);
        tlsSocket.destroy(err);
        return;
      }
      tlsSocket._securePending = false;
      tlsSocket.secureConnecting = false;
      tlsSocket._secureEstablished = true;
      tlsSocket.alpnProtocol = nativeHandle?.alpnProtocol ?? null;
      const requestCert = tlsSocket._requestCert;
      let rejectUnauthorized;
      if (requestCert || (rejectUnauthorized = tlsSocket._rejectUnauthorized)) {
        if (verifyError) {
          tlsSocket.authorized = false;
          tlsSocket.authorizationError = verifyError.code || verifyError.message;
          ctx.server.emit("tlsClientError", verifyError, tlsSocket);
          if (rejectUnauthorized ?? tlsSocket._rejectUnauthorized) {
            tlsSocket.emit("secure", tlsSocket);
            tlsSocket.destroy(verifyError);
            return;
          }
        } else if (requestCert) {
          tlsSocket.authorized = true;
        }
      }
      ctx.connectionListener.call(ctx.server, tlsSocket);
      tlsSocket.resume();
    }
    function onTlsClose() {
      const ctx = this._ctx;
      const raw = ctx.rawSocket;
      const ev = ctx.events;
      if (!ev)
        return;
      raw.removeListener("data", ev[0]);
      raw.removeListener("end", ev[1]);
      raw.removeListener("drain", ev[2]);
      raw.removeListener("close", ev[3]);
    }
    function noop() {}
    function upgradeRawSocketToH2(connectionListener, server, rawSocket) {
      const tlsSocket = new Duplex;
      tlsSocket._ctx = new UpgradeContext(connectionListener, server, rawSocket);
      tlsSocket._read = tlsSocketRead;
      tlsSocket._write = tlsSocketWrite;
      tlsSocket._destroy = tlsSocketDestroy;
      tlsSocket._final = tlsSocketFinal;
      tlsSocket.on("error", noop);
      tlsSocket.alpnProtocol = null;
      tlsSocket.authorized = false;
      tlsSocket.encrypted = true;
      tlsSocket.server = server;
      tlsSocket._requestCert = server._requestCert || false;
      tlsSocket._rejectUnauthorized = server._requestCert ? server._rejectUnauthorized : false;
      let handle, events;
      try {
        [handle, events] = upgradeDuplexToTLS(rawSocket, {
          isServer: true,
          tls: {
            secureContext: server[kSharedCreds]().context,
            requestCert: server._requestCert,
            rejectUnauthorized: server._rejectUnauthorized,
            ALPNProtocols: server.ALPNProtocols ? server.ALPNProtocols.buffer.slice(server.ALPNProtocols.byteOffset, server.ALPNProtocols.byteOffset + server.ALPNProtocols.byteLength) : null
          },
          socket: {
            open: socketOpen,
            data: socketData.bind(tlsSocket),
            end: socketEnd.bind(tlsSocket),
            drain: socketDrain.bind(tlsSocket),
            close: socketClose.bind(tlsSocket),
            error: socketError.bind(tlsSocket),
            timeout: socketTimeout.bind(tlsSocket),
            handshake: socketHandshake.bind(tlsSocket)
          },
          data: {}
        });
      } catch (e) {
        rawSocket.destroy(e);
        tlsSocket.destroy(e);
        return true;
      }
      tlsSocket._ctx.nativeHandle = handle;
      tlsSocket._ctx.events = events;
      rawSocket.on("data", events[0]);
      rawSocket.on("end", events[1]);
      rawSocket.on("drain", events[2]);
      rawSocket.on("close", events[3]);
      tlsSocket.once("close", onTlsClose);
      return true;
    }
    return { upgradeRawSocketToH2 };
  })();
})
