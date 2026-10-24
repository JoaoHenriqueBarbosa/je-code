// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/http.ts` do bun 1.4.2. Não editar à mão.
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
    const { validateInteger, validateObject } = require("internal/validators");
    const httpAgent = require("node:_http_agent");
    const { Agent } = httpAgent;
    const { ClientRequest } = require("node:_http_client");
    const { validateHeaderName, validateHeaderValue, parsers } = require("node:_http_common");
    const { IncomingMessage } = require("node:_http_incoming");
    const { OutgoingMessage } = require("node:_http_outgoing");
    const { Server, ServerResponse } = require("node:_http_server");
    const { METHODS, STATUS_CODES, setMaxHTTPHeaderSize, getMaxHTTPHeaderSize } = require("internal/http");
    if (process.env.NODE_DEBUG)
      require("node:util").debuglog("http");
    const { WebSocket, CloseEvent, MessageEvent } = globalThis;
    function createServer(options, callback) {
      return new Server(options, callback);
    }
    function request(url, options, cb) {
      return new ClientRequest(url, options, cb);
    }
    function get(url, options, cb) {
      const req = request(url, options, cb);
      req.end();
      return req;
    }
    const http_exports = {
      Agent,
      Server,
      METHODS,
      STATUS_CODES,
      createServer,
      ServerResponse,
      IncomingMessage,
      request,
      get,
      get maxHeaderSize() {
        return getMaxHTTPHeaderSize();
      },
      set maxHeaderSize(value) {
        setMaxHTTPHeaderSize(value);
      },
      validateHeaderName,
      validateHeaderValue,
      setMaxIdleHTTPParsers(max) {
        validateInteger(max, "max", 1);
        parsers.max = max;
      },
      setGlobalProxyFromEnv,
      get globalAgent() {
        return httpAgent.globalAgent;
      },
      set globalAgent(value) {
        httpAgent.globalAgent = value;
      },
      ClientRequest,
      OutgoingMessage,
      WebSocket,
      CloseEvent,
      MessageEvent
    };
    function noopRestore() {}
    function setGlobalProxyFromEnv(env = process.env) {
      validateObject(env, "proxyEnv");
      const { parseProxyUrl } = require("internal/http");
      const httpProxy = parseProxyUrl(env, "http:");
      const httpsProxy = parseProxyUrl(env, "https:");
      if (httpProxy === null && httpsProxy === null) {
        return noopRestore;
      }
      if (httpProxy !== null && URL.canParse(httpProxy) === false) {
        throw codes.PROXY_INVALID_CONFIG(`Invalid proxy URL: ${httpProxy}`);
      }
      if (httpsProxy !== null && URL.canParse(httpsProxy) === false) {
        throw codes.PROXY_INVALID_CONFIG(`Invalid proxy URL: ${httpsProxy}`);
      }
      let originalHttpsAgent, originalHttpAgent;
      if (httpProxy !== null) {
        originalHttpAgent = httpAgent.globalAgent;
        httpAgent.globalAgent = new Agent({
          keepAlive: true,
          scheduling: "lifo",
          timeout: 5000,
          proxyEnv: env
        });
      }
      if (httpsProxy !== null) {
        const https = require("node:https");
        originalHttpsAgent = https.globalAgent;
        https.globalAgent = new https.Agent({
          keepAlive: true,
          scheduling: "lifo",
          timeout: 5000,
          proxyEnv: env
        });
      }
      return function restore() {
        if (originalHttpAgent !== undefined) {
          httpAgent.globalAgent = originalHttpAgent;
        }
        if (originalHttpsAgent !== undefined) {
          require("node:https").globalAgent = originalHttpsAgent;
        }
      };
    }
    return http_exports;
  })();
})
