// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/dns.promises.ts` do bun 1.4.2. Não editar à mão.
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
    return require("node:dns").promises;
  })();
})
