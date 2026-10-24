// GERADO por `scripts/gen-node-modules.js` a partir de `src/js/node/_http_common.ts` do bun 1.4.2. Não editar à mão.
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
    const { checkIsHttpToken } = require("internal/validators");
    const FreeList = require("internal/freelist");
    const { methods, allMethods, HTTPParser } = require("native/http_parser");
    const incoming = require("node:_http_incoming");
    const { IncomingMessage, readStart, readStop } = incoming;
    const RegExpPrototypeExec = RegExp.prototype.exec;
    let strictHeaderCharRegex;
    let lenientHeaderCharRegex;
    function checkInvalidHeaderChar(val, lenient = false) {
      if (lenient) {
        lenientHeaderCharRegex ??= /[\x00\x0a\x0d]|[^\x00-\xff]/;
        return RegExpPrototypeExec.call(lenientHeaderCharRegex, val) !== null;
      }
      strictHeaderCharRegex ??= /[^\t\x20-\x7e\x80-\xff]/;
      return RegExpPrototypeExec.call(strictHeaderCharRegex, val) !== null;
    }
    const validateHeaderName = (name, label) => {
      if (typeof name !== "string" || !name || !checkIsHttpToken(name)) {
        throw codes.INVALID_HTTP_TOKEN(label || "Header name", name);
      }
    };
    const validateHeaderValue = (name, value) => {
      if (value === undefined) {
        throw codes.HTTP_INVALID_HEADER_VALUE(value, name);
      }
      if (checkInvalidHeaderChar(value)) {
        throw codes.INVALID_CHAR("header content", name);
      }
    };
    const insecureHTTPParser = (() => false)();
    const kIncomingMessage = Symbol("IncomingMessage");
    const kSkipPendingData = Symbol("SkipPendingData");
    const kOnMessageBegin = HTTPParser.kOnMessageBegin | 0;
    const kOnHeaders = HTTPParser.kOnHeaders | 0;
    const kOnHeadersComplete = HTTPParser.kOnHeadersComplete | 0;
    const kOnBody = HTTPParser.kOnBody | 0;
    const kOnMessageComplete = HTTPParser.kOnMessageComplete | 0;
    const kOnExecute = HTTPParser.kOnExecute | 0;
    const kOnTimeout = HTTPParser.kOnTimeout | 0;
    const MAX_HEADER_PAIRS = 2000;
    function parserOnHeaders(headers, url) {
      const capacity = this.maxHeaderPairs - this._headers.length;
      if (this.maxHeaderPairs <= 0 || capacity >= headers.length) {
        this._headers.push(...headers);
      } else if (capacity > 0) {
        this._headers.push(...headers.slice(0, capacity));
      }
      this._url += url;
    }
    function parserOnHeadersComplete(versionMajor, versionMinor, headers, method, url, statusCode, statusMessage, upgrade, shouldKeepAlive) {
      const parser = this;
      const { socket } = parser;
      if (headers === undefined) {
        headers = parser._headers;
        parser._headers = [];
      }
      if (url === undefined) {
        url = parser._url;
        parser._url = "";
      }
      const ParserIncomingMessage = socket?.server?.[kIncomingMessage] || IncomingMessage;
      const incoming = parser.incoming = new ParserIncomingMessage(socket);
      incoming.httpVersionMajor = versionMajor;
      incoming.httpVersionMinor = versionMinor;
      incoming.httpVersion = `${versionMajor}.${versionMinor}`;
      incoming.joinDuplicateHeaders = socket?.server?.joinDuplicateHeaders || parser.joinDuplicateHeaders;
      incoming.url = url;
      incoming.upgrade = upgrade;
      let n = headers.length;
      const maxHeaderPairs = parser.maxHeaderPairs;
      if (maxHeaderPairs > 0)
        n = Math.min(n, maxHeaderPairs);
      incoming._addHeaderLines(headers, n);
      if (typeof method === "number") {
        incoming.method = allMethods[method];
      } else {
        incoming.statusCode = statusCode;
        incoming.statusMessage = statusMessage;
      }
      return parser.onIncoming(incoming, shouldKeepAlive);
    }
    function parserOnBody(b) {
      const stream = this.incoming;
      if (stream === null || stream[kSkipPendingData])
        return;
      if (!stream._dumped) {
        const ret = stream.push(b);
        if (!ret)
          readStop(this.socket);
      }
    }
    function parserOnMessageComplete() {
      const parser = this;
      const stream = parser.incoming;
      if (stream !== null && !stream[kSkipPendingData]) {
        stream.complete = true;
        const headers = parser._headers;
        const headersLength = headers.length;
        if (headersLength) {
          stream._addHeaderLines(headers, headersLength);
          parser._headers = [];
          parser._url = "";
        }
        stream.push(null);
      }
      readStart(parser.socket);
    }
    const parsers = new FreeList("parsers", 1000, function parsersCb() {
      const parser = new HTTPParser;
      cleanParser(parser);
      parser[kOnHeaders] = parserOnHeaders;
      parser[kOnHeadersComplete] = parserOnHeadersComplete;
      parser[kOnBody] = parserOnBody;
      parser[kOnMessageComplete] = parserOnMessageComplete;
      return parser;
    });
    function closeParserInstance(parser) {
      parser.close();
    }
    function freeParser(parser, req, socket) {
      if (parser) {
        if (parser._consumed)
          parser.unconsume();
        parser.remove();
        cleanParser(parser);
        if (parsers.free(parser) === false) {
          setImmediate(closeParserInstance, parser);
        } else {
          parser.free();
        }
      }
      if (req) {
        req.parser = null;
      }
      if (socket) {
        socket.parser = null;
      }
    }
    function cleanParser(parser) {
      parser._headers = [];
      parser._url = "";
      parser.socket = null;
      parser.incoming = null;
      parser.outgoing = null;
      parser.maxHeaderPairs = MAX_HEADER_PAIRS;
      parser[kOnMessageBegin] = null;
      parser[kOnExecute] = null;
      parser[kOnTimeout] = null;
      parser._consumed = false;
      parser.onIncoming = null;
      parser.joinDuplicateHeaders = null;
    }
    function prepareError(err, parser, rawPacket) {
      err.rawPacket = rawPacket || parser.getCurrentBuffer();
      const reason = err.reason;
      if (typeof reason === "string")
        err.message = `Parse Error: ${reason}`;
    }
    let warnedLenient = false;
    function isLenient() {
      if (insecureHTTPParser && !warnedLenient) {
        warnedLenient = true;
        process.emitWarning("Using insecure HTTP parsing");
      }
      return insecureHTTPParser;
    }
    const kLenientNone = HTTPParser.kLenientNone | 0;
    const kLenientAll = HTTPParser.kLenientAll | 0;
    const kLenientHeaderValueRelaxed = HTTPParser.kLenientHeaderValueRelaxed | 0;
    function calculateLenientFlags(httpValidation, insecureHTTPParserOption) {
      if (httpValidation === "strict") {
        return kLenientNone;
      } else if (httpValidation === "relaxed") {
        return kLenientHeaderValueRelaxed;
      } else if (httpValidation === "insecure") {
        return kLenientAll;
      }
      const lenient = insecureHTTPParserOption === undefined ? isLenient() : insecureHTTPParserOption;
      return lenient ? kLenientAll : kLenientNone;
    }
    return {
      validateHeaderName,
      validateHeaderValue,
      _checkIsHttpToken: checkIsHttpToken,
      _checkInvalidHeaderChar: checkInvalidHeaderChar,
      chunkExpression: /(?:^|\W)chunked(?:$|\W)/i,
      continueExpression: /(?:^|\W)100-continue(?:$|\W)/i,
      CRLF: `\r
    `,
      freeParser,
      methods,
      parsers,
      kIncomingMessage,
      kSkipPendingData,
      HTTPParser,
      isLenient,
      calculateLenientFlags,
      prepareError
    };
  })();
})
