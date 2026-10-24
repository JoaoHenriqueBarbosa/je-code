// O que os módulos embutidos escritos em JavaScript (`events`, `stream`) dividem: erros com código no formato do
// `node:internal/errors` do bun, os validadores de `internal/validators` (no bun são funções em C++, `NodeValidator.cpp`),
// `internal/abort_listener`, `internal/shared` (só o que o `stream` usa) e `internal/primordials`. Roda uma vez por
// programa; o resultado é o objeto que `node_events.js` e `node_stream_modules.js` recebem como `host.internal`.
//
// `host` traz `throwNodeError(tipo, código, mensagem)`, `inspect(valor, opções)` e `requireBuiltin(nome)`.
(function (host) {
  "use strict";

  const ArrayIsArray = Array.isArray;
  const NumberIsInteger = Number.isInteger;
  const NumberIsNaN = Number.isNaN;
  const ObjectCreate = Object.create;
  const ObjectFreeze = Object.freeze;
  const ReflectApply = Reflect.apply;
  const SymbolDispose = Symbol.dispose;
  const queueMicrotaskNative = queueMicrotask;

  // ---- erros com código ----

  function makeError(kind, code, message) {
    try {
      host.throwNodeError(kind, code, message);
    } catch (error) {
      return error;
    }
  }

  const kTypes = ["string", "function", "number", "object", "Function", "Object", "boolean", "bigint", "symbol"];
  const classRegExp = /^([A-Z][a-z0-9]*)+$/;

  function determineSpecificType(value) {
    if (value == null) return "" + value;
    if (typeof value === "function") return `function ${value.name}`;
    if (typeof value === "object") {
      if (value.constructor?.name) return `an instance of ${value.constructor.name}`;
      return host.inspect(value, { depth: -1 });
    }
    let inspected = host.inspect(value, { colors: false });
    if (inspected.length > 28) inspected = `${inspected.slice(0, 25)}...`;
    return `type ${typeof value} (${inspected})`;
  }

  function invalidArgType(name, expected, actual) {
    // Os validadores do bun são C++: o nome ausente vira o texto "undefined" (`validateFunction(fn)` sem nome dá
    // `The "undefined" argument must be of type function`, medido no bun 1.4.2).
    name = `${name}`;
    // Um texto com a lista pronta ("string or URL", como o `streams.ts` do bun passa) entra verbatim.
    const verbatim = typeof expected === "string" && expected.includes(" or ");
    if (!ArrayIsArray(expected)) expected = [expected];
    let message = "The ";
    if (name.endsWith(" argument")) {
      message += `${name} `;
    } else {
      message += `"${name}" ${name.includes(".") ? "property" : "argument"} `;
    }
    message += "must be ";
    if (verbatim) return makeError("TypeError", "ERR_INVALID_ARG_TYPE", `${message}of type ${expected[0]}. Received ${determineSpecificType(actual)}`);
    const types = [];
    const instances = [];
    const other = [];
    for (const value of expected) {
      if (kTypes.includes(value)) types.push(value.toLowerCase());
      else if (value === "AbortSignal") types.push(value);
      else if (classRegExp.exec(value) !== null) instances.push(value);
      else other.push(value);
    }
    if (instances.length > 0) {
      const position = types.indexOf("object");
      if (position !== -1) {
        types.splice(position, 1);
        instances.push("Object");
      }
    }
    if (types.length > 0) {
      if (types.length > 2) {
        const last = types.pop();
        message += `one of type ${types.join(", ")}, or ${last}`;
      } else if (types.length === 2) {
        message += `one of type ${types[0]} or ${types[1]}`;
      } else {
        message += `of type ${types[0]}`;
      }
      if (instances.length > 0 || other.length > 0) message += " or ";
    }
    if (instances.length > 0) {
      if (instances.length > 2) {
        const last = instances.pop();
        message += `an instance of ${instances.join(", ")}, or ${last}`;
      } else {
        message += `an instance of ${instances[0]}`;
        if (instances.length === 2) message += ` or ${instances[1]}`;
      }
      if (other.length > 0) message += " or ";
    }
    if (other.length > 0) {
      if (other.length > 2) {
        const last = other.pop();
        message += `one of ${other.join(", ")}, or ${last}`;
      } else if (other.length === 2) {
        message += `one of ${other[0]} or ${other[1]}`;
      } else {
        if (other[0].toLowerCase() !== other[0]) message += "an ";
        message += `${other[0]}`;
      }
    }
    message += `. Received ${determineSpecificType(actual)}`;
    return makeError("TypeError", "ERR_INVALID_ARG_TYPE", message);
  }

  function outOfRange(name, range, received) {
    return makeError("RangeError", "ERR_OUT_OF_RANGE", `The value of "${name}" is out of range. It must be ${range}. Received ${received}`);
  }

  // `$ERR_INVALID_ARG_VALUE` e a variante `_RangeError` do bun: só o tipo do erro muda.
  function invalidArgValue(kind) {
    return (name, value, reason = "is invalid") => {
      let inspected = host.inspect(value, undefined);
      if (inspected.length > 128) inspected = `${inspected.slice(0, 128)}...`;
      const type = name.includes(".") ? "property" : "argument";
      return makeError(kind, "ERR_INVALID_ARG_VALUE", `The ${type} '${name}' ${reason}. Received ${inspected}`);
    };
  }

  // `$ERR_*` do bun: cada código é uma função que devolve o erro (o chamador faz `throw`).
  const codes = {
    INVALID_ARG_TYPE: invalidArgType,
    OUT_OF_RANGE: outOfRange,
    INVALID_ARG_VALUE: invalidArgValue("TypeError"),
    INVALID_ARG_VALUE_RangeError: invalidArgValue("RangeError"),
    INVALID_STATE: message => makeError("Error", "ERR_INVALID_STATE", `Invalid state: ${message}`),
    OPERATION_FAILED: message => makeError("Error", "ERR_OPERATION_FAILED", `Operation failed: ${message}`),
    // O `$ERR_INVALID_RETURN_VALUE` do bun descreve o valor com o determineSpecificType (`type number (5)`, `an instance
    // of Object`, `null`), não com o `instance of X` do Node; medido no bun 1.4.2.
    INVALID_RETURN_VALUE(input, name, value) {
      return makeError(
        "TypeError",
        "ERR_INVALID_RETURN_VALUE",
        `Expected ${input} to be returned from the "${name}" function but got ${determineSpecificType(value)}.`,
      );
    },
    MISSING_ARGS(...args) {
      let message = "The ";
      const wrap = a => `"${a}"`;
      args = args.map(a => (ArrayIsArray(a) ? a.map(wrap).join(" or ") : wrap(a)));
      switch (args.length) {
        case 1:
          message += `${args[0]} argument`;
          break;
        case 2:
          message += `${args[0]} and ${args[1]} arguments`;
          break;
        default: {
          const last = args.pop();
          message += `${args.join(", ")}, and ${last} arguments`;
        }
      }
      return makeError("TypeError", "ERR_MISSING_ARGS", `${message} must be specified`);
    },
    AMBIGUOUS_ARGUMENT: (name, reason) => makeError("TypeError", "ERR_AMBIGUOUS_ARGUMENT", `The "${name}" argument is ambiguous. ${reason}`),
    UNAVAILABLE_DURING_EXIT: () => makeError("Error", "ERR_UNAVAILABLE_DURING_EXIT", "Cannot call function in process exit handler"),
    CONSTRUCT_CALL_REQUIRED: message => makeError("TypeError", "ERR_CONSTRUCT_CALL_REQUIRED", message),
    ILLEGAL_CONSTRUCTOR: () => makeError("TypeError", "ERR_ILLEGAL_CONSTRUCTOR", "Illegal constructor"),
    // Os de URL do bun: a mensagem é o argumento como veio (`$ERR_INVALID_URL_SCHEME("file")` dá a mensagem "file").
    INVALID_URI: () => makeError("URIError", "ERR_INVALID_URI", "URI malformed"),
    INVALID_URL_SCHEME: message => makeError("TypeError", "ERR_INVALID_URL_SCHEME", message),
    INVALID_FILE_URL_HOST: message => makeError("TypeError", "ERR_INVALID_FILE_URL_HOST", message),
    INVALID_FILE_URL_PATH: message => makeError("TypeError", "ERR_INVALID_FILE_URL_PATH", message),
    INVALID_URL(input, base) {
      const error = makeError("TypeError", "ERR_INVALID_URL", "Invalid URL");
      error.input = input;
      if (base !== undefined && base !== null) error.base = base;
      return error;
    },
    UNKNOWN_ENCODING: encoding => makeError("TypeError", "ERR_UNKNOWN_ENCODING", `Unknown encoding: ${encoding}`),
    STREAM_DESTROYED: name => makeError("Error", "ERR_STREAM_DESTROYED", `Cannot call ${name} after a stream was destroyed`),
    STREAM_ALREADY_FINISHED: name => makeError("Error", "ERR_STREAM_ALREADY_FINISHED", `Cannot call ${name} after a stream was finished`),
    STREAM_NULL_VALUES: () => makeError("TypeError", "ERR_STREAM_NULL_VALUES", "May not write null values to stream"),
    STREAM_PREMATURE_CLOSE: () => makeError("Error", "ERR_STREAM_PREMATURE_CLOSE", "Premature close"),
    STREAM_PUSH_AFTER_EOF: () => makeError("Error", "ERR_STREAM_PUSH_AFTER_EOF", "stream.push() after EOF"),
    STREAM_UNSHIFT_AFTER_END_EVENT: () => makeError("Error", "ERR_STREAM_UNSHIFT_AFTER_END_EVENT", "stream.unshift() after end event"),
    STREAM_WRITE_AFTER_END: () => makeError("Error", "ERR_STREAM_WRITE_AFTER_END", "write after end"),
    STREAM_CANNOT_PIPE: () => makeError("Error", "ERR_STREAM_CANNOT_PIPE", "Cannot pipe, not readable"),
    STREAM_UNABLE_TO_PIPE: () => makeError("Error", "ERR_STREAM_UNABLE_TO_PIPE", "Cannot pipe to a closed or destroyed stream"),
    MULTIPLE_CALLBACK: () => makeError("Error", "ERR_MULTIPLE_CALLBACK", "Callback called multiple times"),
    METHOD_NOT_IMPLEMENTED: name => makeError("Error", "ERR_METHOD_NOT_IMPLEMENTED", `The ${name} method is not implemented`),
    UNHANDLED_ERROR: stringified => makeError("Error", "ERR_UNHANDLED_ERROR", `Unhandled error. (${stringified})`),
    INVALID_ASYNC_ID: (type, id) => makeError("RangeError", "ERR_INVALID_ASYNC_ID", `Invalid ${type} value: ${id}`),
    ASYNC_TYPE: type => makeError("TypeError", "ERR_ASYNC_TYPE", `Invalid name for async "type": ${type}`),
    ASYNC_CALLBACK: name => makeError("TypeError", "ERR_ASYNC_CALLBACK", `${name} must be a function`),
    // Os de `node:net` (ErrorCode.cpp do bun 1.4.2).
    SOCKET_CLOSED: () => makeError("Error", "ERR_SOCKET_CLOSED", "Socket is closed"),
    SOCKET_CLOSED_BEFORE_CONNECTION: () => makeError("Error", "ERR_SOCKET_CLOSED_BEFORE_CONNECTION", "Socket closed before the connection was established"),
    SOCKET_CONNECTION_TIMEOUT: () => makeError("Error", "ERR_SOCKET_CONNECTION_TIMEOUT", "Socket connection timeout"),
    TLS_HANDSHAKE_TIMEOUT: () => makeError("Error", "ERR_TLS_HANDSHAKE_TIMEOUT", "TLS handshake timeout"),
    SERVER_NOT_RUNNING: () => makeError("Error", "ERR_SERVER_NOT_RUNNING", "Server is not running."),
    SERVER_ALREADY_LISTEN: () => makeError("Error", "ERR_SERVER_ALREADY_LISTEN", "Listen method has been called more than once without closing."),
    IP_BLOCKED: ip => makeError("Error", "ERR_IP_BLOCKED", `IP(${ip}) is blocked by net.BlockList`),
    INVALID_FD_TYPE: type => makeError("TypeError", "ERR_INVALID_FD_TYPE", `Unsupported fd type: ${type}`),
    INVALID_IP_ADDRESS: ip => makeError("TypeError", "ERR_INVALID_IP_ADDRESS", `Invalid IP address: ${ip}`),
    INVALID_ADDRESS_FAMILY(family, host, port) {
      const error = makeError("RangeError", "ERR_INVALID_ADDRESS_FAMILY", `Invalid address family: ${family} ${host}:${port}`);
      error.host = host;
      error.port = port;
      return error;
    },
    SOCKET_BAD_PORT: message => makeError("RangeError", "ERR_SOCKET_BAD_PORT", message),
    TLS_ALPN_CALLBACK_INVALID_RESULT: message => makeError("TypeError", "ERR_TLS_ALPN_CALLBACK_INVALID_RESULT", message),
    // Os de `node:tls` (ErrorCode.cpp do bun 1.4.2).
    TLS_ALPN_CALLBACK_WITH_PROTOCOLS: () => makeError("TypeError", "ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS", "The ALPNCallback and ALPNProtocols TLS options are mutually exclusive"),
    TLS_CERT_ALTNAME_FORMAT: () => makeError("SyntaxError", "ERR_TLS_CERT_ALTNAME_FORMAT", "Invalid subject alternative name string"),
    TLS_CERT_ALTNAME_INVALID(reason, host, cert) {
      const error = makeError("Error", "ERR_TLS_CERT_ALTNAME_INVALID", `Hostname/IP does not match certificate's altnames: ${reason}`);
      error.reason = reason;
      error.host = host;
      error.cert = cert;
      return error;
    },
    TLS_INVALID_PROTOCOL_VERSION: (version, kind) => makeError("TypeError", "ERR_TLS_INVALID_PROTOCOL_VERSION", `${version} is not a valid ${kind} TLS protocol version`),
    TLS_INVALID_STATE: () => makeError("Error", "ERR_TLS_INVALID_STATE", "TLS socket connection must be securely established"),
    TLS_RENEGOTIATION_DISABLED: () => makeError("Error", "ERR_TLS_RENEGOTIATION_DISABLED", "TLS session renegotiation disabled for this socket"),
    TLS_RENEGOTIATION_UNSUPPORTED: () => makeError("Error", "ERR_TLS_RENEGOTIATION_UNSUPPORTED", "TLS session renegotiation is unsupported by this TLS implementation"),
    TLS_SNI_FROM_SERVER: () => makeError("Error", "ERR_TLS_SNI_FROM_SERVER", "Cannot issue SNI from a TLS server-side socket"),
    // Os de `node:http` (ErrorCode.cpp do bun 1.4.2); o que não tem molde lá leva o primeiro argumento como mensagem.
    HTTP_BODY_NOT_ALLOWED: () => makeError("Error", "ERR_HTTP_BODY_NOT_ALLOWED", "Adding content for this request method or response status is not allowed."),
    HTTP_CONTENT_LENGTH_MISMATCH: message => makeError("Error", "ERR_HTTP_CONTENT_LENGTH_MISMATCH", String(message)),
    HTTP_HEADERS_SENT: action => makeError("Error", "ERR_HTTP_HEADERS_SENT", `Cannot ${action} headers after they are sent to the client`),
    HTTP_INVALID_HEADER_VALUE: (value, name) => makeError("TypeError", "ERR_HTTP_INVALID_HEADER_VALUE", `Invalid value "${value}" for header "${name}"`),
    HTTP_INVALID_STATUS_CODE: code => makeError("RangeError", "ERR_HTTP_INVALID_STATUS_CODE", `Invalid status code: ${code}`),
    HTTP_REQUEST_TIMEOUT: message => makeError("Error", "ERR_HTTP_REQUEST_TIMEOUT", String(message)),
    HTTP_SOCKET_ASSIGNED: () => makeError("Error", "ERR_HTTP_SOCKET_ASSIGNED", "Socket already assigned"),
    HTTP_TRAILER_INVALID: message => makeError("Error", "ERR_HTTP_TRAILER_INVALID", String(message)),
    INVALID_CHAR: (what, value) => makeError("TypeError", "ERR_INVALID_CHAR", `Invalid character in ${what}${value === undefined ? "" : ` ["${value}"]`}`),
    INVALID_HTTP_TOKEN: (name, value) => makeError("TypeError", "ERR_INVALID_HTTP_TOKEN", `${name} must be a valid HTTP token ["${value}"]`),
    INVALID_PROTOCOL: (actual, expected) => makeError("TypeError", "ERR_INVALID_PROTOCOL", `Protocol "${actual}" not supported. Expected "${expected}"`),
    PROXY_INVALID_CONFIG: message => makeError("Error", "ERR_PROXY_INVALID_CONFIG", String(message)),
    PROXY_TUNNEL: message => makeError("Error", "ERR_PROXY_TUNNEL", String(message)),
    UNESCAPED_CHARACTERS: name => makeError("TypeError", "ERR_UNESCAPED_CHARACTERS", `${name} contains unescaped characters`),
  };
  // Os erros do analisador HTTP (`HPE_*`): o argumento é a mensagem.
  for (const name of [
    "CHUNK_EXTENSIONS_OVERFLOW", "CLOSED_CONNECTION", "HEADER_OVERFLOW", "INTERNAL", "INVALID_CHUNK_SIZE", "INVALID_CONSTANT",
    "INVALID_CONTENT_LENGTH", "INVALID_EOF_STATE", "INVALID_HEADER_TOKEN", "INVALID_METHOD", "INVALID_TRANSFER_ENCODING",
    "INVALID_VERSION", "LF_EXPECTED", "PAUSED_H2_UPGRADE", "STRICT", "UNEXPECTED_CONTENT_LENGTH",
  ]) {
    codes[`HPE_${name}`] = message => makeError("Error", `HPE_${name}`, String(message));
  }
  // Os que o bun declara só com a mensagem pronta (`$ERR_FS_CP_EEXIST(message)`): o argumento é a mensagem.
  for (const [name, kind] of [
    ["FS_CP_DIR_TO_NON_DIR", "Error"],
    ["FS_CP_EEXIST", "Error"],
    ["FS_CP_EINVAL", "Error"],
    ["FS_CP_FIFO_PIPE", "Error"],
    ["FS_CP_NON_DIR_TO_DIR", "Error"],
    ["FS_CP_SOCKET", "Error"],
    ["FS_CP_SYMLINK_TO_SUBDIRECTORY", "Error"],
    ["FS_CP_UNKNOWN", "Error"],
    ["FS_EISDIR", "Error"],
    ["INCOMPATIBLE_OPTION_PAIR", "TypeError"],
  ]) {
    codes[name] = message => makeError(kind, `ERR_${name}`, message);
  }

  // `AbortError` do bun: o protótipo traz `name` e `code`, e `constructor` continua o de `Error`.
  const abortErrorPrototype = ObjectCreate(Error.prototype, {
    name: { value: "AbortError", writable: true, configurable: true },
    code: { value: "ABORT_ERR", writable: true, configurable: true },
  });
  function makeAbortError(message = "The operation was aborted", options = undefined) {
    const error = new Error(message, options);
    Object.setPrototypeOf(error, abortErrorPrototype);
    return error;
  }

  // ---- internal/validators ----

  const kValidateObjectNone = 0;
  const kValidateObjectAllowNullable = 1 << 0;
  const kValidateObjectAllowArray = 1 << 1;
  const kValidateObjectAllowFunction = 1 << 2;

  function validateString(value, name) {
    if (typeof value !== "string") throw invalidArgType(name, "string", value);
  }

  function validateFunction(value, name) {
    if (typeof value !== "function") throw invalidArgType(name, "function", value);
  }

  function validateBoolean(value, name) {
    if (typeof value !== "boolean") throw invalidArgType(name, "boolean", value);
  }

  function validateNumber(value, name, min = undefined, max) {
    if (typeof value !== "number") throw invalidArgType(name, "number", value);
    if (
      (min != null && value < min) ||
      (max != null && value > max) ||
      ((min != null || max != null) && NumberIsNaN(value))
    ) {
      throw outOfRange(
        name,
        `${min != null ? `>= ${min}` : ""}${min != null && max != null ? " && " : ""}${max != null ? `<= ${max}` : ""}`,
        value,
      );
    }
  }

  function validateUint32(value, name, positive = false) {
    if (typeof value !== "number") throw invalidArgType(name, "number", value);
    if (value % 1 !== 0) throw outOfRange(name, "an integer", value);
    const min = positive ? 1 : 0;
    if (value < min || value > 4294967295) throw outOfRange(name, `>= ${min} && <= 4294967295`, value);
  }

  function validateInteger(value, name, min = Number.MIN_SAFE_INTEGER, max = Number.MAX_SAFE_INTEGER) {
    if (typeof value !== "number") throw invalidArgType(name, "number", value);
    if (!NumberIsInteger(value)) throw outOfRange(name, "an integer", value);
    if (value < min || value > max) throw outOfRange(name, `>= ${min} && <= ${max}`, value);
  }

  function validateInt32(value, name, min = -2147483648, max = 2147483647) {
    if (typeof value !== "number") throw invalidArgType(name, "number", value);
    if (!NumberIsInteger(value)) throw outOfRange(name, "an integer", value);
    if (value < min || value > max) throw outOfRange(name, `>= ${min} && <= ${max}`, value);
  }

  function validateObject(value, name, options = kValidateObjectNone) {
    if (options === kValidateObjectNone) {
      if (value === null || ArrayIsArray(value) || typeof value !== "object") throw invalidArgType(name, "object", value);
      return;
    }
    const throwOnNullable = (options & kValidateObjectAllowNullable) === 0;
    if (throwOnNullable && value === null) throw invalidArgType(name, "object", value);
    if ((options & kValidateObjectAllowArray) === 0 && ArrayIsArray(value)) throw invalidArgType(name, "object", value);
    if (
      typeof value !== "object" &&
      ((options & kValidateObjectAllowFunction) === 0 || typeof value !== "function")
    ) {
      throw invalidArgType(name, "object", value);
    }
  }

  // `jsFunction_validatePort` do bun (NodeValidator.cpp): número ou texto inteiro de 0 (ou 1) a 65535.
  function validatePort(port, name = "Port", allowZero = true) {
    const bad = () => codes.SOCKET_BAD_PORT(`${name} should be ${allowZero ? ">=" : ">"} 0 and < 65536. Received ${String(port)}`);
    if (typeof port !== "number" && typeof port !== "string") throw bad();
    if (typeof port === "string" && port.trim().length === 0) throw bad();
    const value = Number(port);
    if (Number.isNaN(value) || !Number.isFinite(value) || value % 1 !== 0 || value < 0 || value > 0xffff || (value === 0 && !allowZero)) throw bad();
    return port;
  }

  function validateAbortSignal(signal, name) {
    if (signal !== undefined && (signal === null || typeof signal !== "object" || !("aborted" in signal))) {
      throw invalidArgType(name, "AbortSignal", signal);
    }
  }

  // `checkIsHttpToken` e `validateLinkHeaderValue` do `internal/validators.ts` do bun 1.4.2.
  const tokenRegExp = /^[\^_`a-zA-Z\-0-9!#$%&'*+.|~]+$/;
  function checkIsHttpToken(val) {
    return tokenRegExp.exec(val) !== null;
  }

  const linkValueRegExp = /^(?:<[^>]*>)(?:\s*;\s*[^;"\s=]+(?:=(")?[^;"\s]*\1)?)*$/;
  const linkValueForbiddenCharsRegExp = /[\r\n]/;
  function validateLinkHeaderFormat(value, name) {
    if (typeof value === "undefined" || !linkValueRegExp.exec(value) || linkValueForbiddenCharsRegExp.exec(value) !== null) {
      throw codes.INVALID_ARG_VALUE(name, value, `must be an array or string of format "</styles.css>; rel=preload; as=style"`);
    }
  }

  function validateLinkHeaderValue(hints) {
    if (typeof hints === "string") {
      validateLinkHeaderFormat(hints, "hints");
      return hints;
    }
    if (ArrayIsArray(hints)) {
      let result = "";
      for (let i = 0; i < hints.length; i++) {
        validateLinkHeaderFormat(hints[i], "hints");
        result += hints[i];
        if (i !== hints.length - 1) result += ", ";
      }
      return result;
    }
    throw codes.INVALID_ARG_VALUE("hints", hints, `must be an array or string of format "</styles.css>; rel=preload; as=style"`);
  }

  // `jsFunction_validateBuffer` (NodeValidator.cpp): indefinido passa; o resto precisa ser view de buffer.
  function validateBuffer(buffer, name) {
    if (buffer !== undefined && !ArrayBuffer.isView(buffer)) {
      const label = name === undefined ? "buffer" : `${name}`;
      throw makeError("TypeError", "ERR_INVALID_ARG_TYPE", `The "${label}" argument must be an instance of Buffer, TypedArray, or DataView. Received ${determineSpecificType(buffer)}`);
    }
  }

  function validateArray(value, name, minLength = 0) {
    if (!ArrayIsArray(value)) throw invalidArgType(name, "Array", value);
    if (value.length < minLength) {
      throw codes.INVALID_ARG_VALUE(name, value, `must be longer than ${minLength}`);
    }
  }

  function validateOneOf(value, name, oneOf) {
    if (!oneOf.includes(value)) {
      const allowed = oneOf.map(v => (typeof v === "string" ? `'${v}'` : String(v))).join(", ");
      throw codes.INVALID_ARG_VALUE(name, value, `must be one of: ${allowed}`);
    }
  }

  const fileURLToPath = url => host.requireBuiltin("node:url").fileURLToPath(url);

  // Um caminho de texto ou URL, resolvido para absoluto (`getValidatedPath` do bun).
  function getValidatedPath(path) {
    if (path instanceof URL) return fileURLToPath(path);
    if (typeof path !== "string") throw invalidArgType("path", "string or URL", path);
    if (path.startsWith("file:")) return fileURLToPath(path);
    return host.requireBuiltin("node:path").resolve(path);
  }

  function throwIfNullBytesInFileName(filename) {
    if (filename.indexOf("\u0000") !== -1) throw codes.INVALID_ARG_VALUE("path", filename, "string without null bytes");
  }

  // O `getValidatedFsPath` do bun (o do Node): URL vira caminho, texto e Uint8Array entram como vieram, sem byte nulo.
  function getValidatedFsPath(value, name = "path") {
    if (value instanceof URL) value = fileURLToPath(value);
    const bytes = typeof value === "string" ? value.indexOf("\u0000") : value instanceof Uint8Array ? value.indexOf(0) : undefined;
    if (bytes === undefined) throw invalidArgType(name, ["string", "Buffer", "URL"], value);
    if (bytes !== -1) throw codes.INVALID_ARG_VALUE(name, value, "must be a string, Uint8Array, or URL without null bytes");
    return value;
  }

  const validators = {
    kValidateObjectNone,
    kValidateObjectAllowNullable,
    kValidateObjectAllowArray,
    kValidateObjectAllowFunction,
    validateObject,
    validateInteger,
    validateInt32,
    validateUint32,
    validateNumber,
    validateString,
    validateFunction,
    validateBoolean,
    validateAbortSignal,
    validatePort,
    validateArray,
    checkIsHttpToken,
    validateLinkHeaderValue,
    validateBuffer,
    isUint8Array: value => value instanceof Uint8Array,
    validateOneOf,
    getValidatedPath,
    throwIfNullBytesInFileName,
    getValidatedFsPath,
  };

  // ---- internal/errors ----

  class SafeArrayIterator {
    constructor(array) {
      this.iterator = array[Symbol.iterator]();
    }
    next() {
      return this.iterator.next();
    }
    [Symbol.iterator]() {
      return this;
    }
  }

  function aggregateTwoErrors(innerError, outerError) {
    if (innerError && outerError && innerError !== outerError) {
      const outerErrors = outerError.errors;
      if (ArrayIsArray(outerErrors)) {
        // If `outerError` is already an `AggregateError`.
        outerErrors.push(innerError);
        return outerError;
      }
      const err = new AggregateError(new SafeArrayIterator([outerError, innerError]), outerError.message);
      err.code = outerError.code;
      return err;
    }
    return innerError || outerError;
  }

  // ---- internal/shared ----

  const kEmptyObject = ObjectFreeze(ObjectCreate(null));
  const kResistStopPropagation = Symbol("kResistStopPropagation");

  function resistStopPropagation(options) {
    options[kResistStopPropagation] = true;
    return options;
  }

  const kCustomPromisifyArgsSymbol = Symbol("customPromisifyArgs");

  let utilModule;
  class ErrnoException extends Error {
    // Os `errno: number; syscall: string;` do TypeScript viram campos de classe: nascem antes do `code` (medido:
    // `message,errno,syscall,code`).
    errno;
    syscall;
    constructor(err, syscall, original) {
      utilModule ??= host.requireBuiltin("node:util");
      const code = utilModule.getSystemErrorName(err);
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

  class NotImplementedError extends Error {
    // O `code: string;` do TypeScript vira campo de classe: `code` nasce antes de `name` (medido: `code,name`).
    code;
    constructor(feature, issue, extra) {
      super(
        feature +
          " is not yet implemented in Bun." +
          (issue ? " Track the status & thumbs up the issue: https://github.com/oven-sh/bun/issues/" + issue : "") +
          (extra ? ". " + extra : ""),
      );
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

  // O bun esconde da pilha trocando o `name` da função.
  function hideFromStack(...fns) {
    for (const fn of fns) {
      Object.defineProperty(fn, "name", {
        value: "::bunternal::",
      });
    }
  }

  const nodeModulesRE = /[\\/]node_modules[\\/]/;
  const ErrorCaptureStackTrace = Error.captureStackTrace;
  function returnStackFrames(_err, frames) {
    return frames;
  }

  // O `IsInsideNodeModules` do node: o primeiro frame de usuário está dentro de `node_modules`? Protegido para que um
  // `Error.*` adulterado nunca escape para quem chama (o `url.parse`).
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
    } catch {
    } finally {
      try {
        Error.stackTraceLimit = prevLimit;
        Error.prepareStackTrace = prevPrepare;
      } catch {}
    }
    if (!ArrayIsArray(frames)) return false;
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

  function once(callback, { preserveReturnValue = false } = kEmptyObject) {
    let called = false;
    let returnValue;
    return function (...args) {
      if (called) return returnValue;
      called = true;
      const fn = callback;
      // Drop the reference so the wrapper cannot keep the callback's closure alive once it has run.
      callback = undefined;
      const result = ReflectApply(fn, this, args);
      returnValue = preserveReturnValue ? result : undefined;
      return result;
    };
  }

  // ---- internal/abort_listener ----

  function addAbortListener(signal, listener) {
    if (signal === undefined) throw invalidArgType("signal", "AbortSignal", signal);
    validateAbortSignal(signal, "signal");
    validateFunction(listener, "listener");
    let removeEventListener;
    if (signal.aborted) {
      queueMicrotaskNative(() => listener());
    } else {
      signal.addEventListener("abort", listener, resistStopPropagation({ __proto__: null, once: true }));
      removeEventListener = () => {
        signal.removeEventListener("abort", listener);
      };
    }
    return {
      __proto__: null,
      [SymbolDispose]() {
        removeEventListener?.();
      },
    };
  }

  return {
    makeError,
    codes,
    determineSpecificType,
    makeAbortError,
    validators,
    aggregateTwoErrors,
    SafeArrayIterator,
    SafeSet: Set,
    kEmptyObject,
    kResistStopPropagation,
    resistStopPropagation,
    kCustomPromisifyArgsSymbol,
    ErrnoException,
    throwNotImplemented,
    isInsideNodeModules,
    once,
    addAbortListener,
    toClass: host.toClass,
  };
})
