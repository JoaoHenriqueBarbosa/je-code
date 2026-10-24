// GERADO por `scripts/gen-node-crypto.js` a partir de `src/js/node/crypto.ts` do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";
  const { validators, codes: baseCodes, makeError, toClass, kCustomPromisifyArgsSymbol, throwNotImplemented } = host.internal;
  const native = host.crypto;
  const { isAnyArrayBuffer } = host.requireBuiltin("node:util/types");

  const codes = {
    ...baseCodes,
    UNKNOWN_ENCODING: encoding => makeError("TypeError", "ERR_UNKNOWN_ENCODING", "Unknown encoding: " + encoding),
    INTERNAL_ASSERTION: message => makeError("Error", "ERR_INTERNAL_ASSERTION", message),
  };

  const cryptoConstants =
  {
    "OPENSSL_VERSION_NUMBER": 269488255,
    "SSL_OP_ALL": 4,
    "SSL_OP_ALLOW_NO_DHE_KEX": 0,
    "SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION": 0,
    "SSL_OP_CIPHER_SERVER_PREFERENCE": 4194304,
    "SSL_OP_CISCO_ANYCONNECT": 0,
    "SSL_OP_COOKIE_EXCHANGE": 0,
    "SSL_OP_CRYPTOPRO_TLSEXT_BUG": 0,
    "SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS": 0,
    "SSL_OP_LEGACY_SERVER_CONNECT": 4,
    "SSL_OP_NO_COMPRESSION": 0,
    "SSL_OP_NO_ENCRYPT_THEN_MAC": 0,
    "SSL_OP_NO_QUERY_MTU": 4096,
    "SSL_OP_NO_RENEGOTIATION": 0,
    "SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION": 0,
    "SSL_OP_NO_SSLv2": 0,
    "SSL_OP_NO_SSLv3": 0,
    "SSL_OP_NO_TICKET": 16384,
    "SSL_OP_NO_TLSv1": 67108864,
    "SSL_OP_NO_TLSv1_1": 268435456,
    "SSL_OP_NO_TLSv1_2": 134217728,
    "SSL_OP_NO_TLSv1_3": 536870912,
    "SSL_OP_PRIORITIZE_CHACHA": 0,
    "SSL_OP_TLS_ROLLBACK_BUG": 0,
    "SSL_OP_MICROSOFT_SESS_ID_BUG": 0,
    "SSL_OP_NETSCAPE_CHALLENGE_BUG": 0,
    "SSL_OP_NETSCAPE_REUSE_CIPHER_CHANGE_BUG": 0,
    "SSL_OP_SSLREF2_REUSE_CERT_TYPE_BUG": 0,
    "SSL_OP_MICROSOFT_BIG_SSLV3_BUFFER": 0,
    "SSL_OP_MSIE_SSLV2_RSA_PADDING": 0,
    "SSL_OP_SSLEAY_080_CLIENT_DH_BUG": 0,
    "SSL_OP_TLS_D5_BUG": 0,
    "SSL_OP_TLS_BLOCK_PADDING_BUG": 0,
    "SSL_OP_SINGLE_ECDH_USE": 0,
    "SSL_OP_SINGLE_DH_USE": 0,
    "SSL_OP_EPHEMERAL_RSA": 0,
    "SSL_OP_PKCS1_CHECK_1": 0,
    "SSL_OP_PKCS1_CHECK_2": 0,
    "SSL_OP_NETSCAPE_CA_DN_BUG": 0,
    "SSL_OP_NETSCAPE_DEMO_CIPHER_CHANGE_BUG": 0,
    "ENGINE_METHOD_RSA": 1,
    "ENGINE_METHOD_DSA": 2,
    "ENGINE_METHOD_DH": 4,
    "ENGINE_METHOD_RAND": 8,
    "ENGINE_METHOD_CIPHERS": 64,
    "ENGINE_METHOD_DIGESTS": 128,
    "ENGINE_METHOD_PKEY_METHS": 512,
    "ENGINE_METHOD_PKEY_ASN1_METHS": 1024,
    "ENGINE_METHOD_EC": 2048,
    "ENGINE_METHOD_ALL": 65535,
    "ENGINE_METHOD_NONE": 0,
    "DH_CHECK_P_NOT_SAFE_PRIME": 2,
    "DH_CHECK_P_NOT_PRIME": 1,
    "DH_UNABLE_TO_CHECK_GENERATOR": 4,
    "DH_NOT_SUITABLE_GENERATOR": 8,
    "RSA_PKCS1_PADDING": 1,
    "RSA_NO_PADDING": 3,
    "RSA_PKCS1_OAEP_PADDING": 4,
    "RSA_X931_PADDING": 5,
    "RSA_PKCS1_PSS_PADDING": 6,
    "RSA_PSS_SALTLEN_DIGEST": -1,
    "RSA_PSS_SALTLEN_MAX_SIGN": -2,
    "RSA_PSS_SALTLEN_AUTO": -2,
    "defaultCoreCipherList": "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA",
    "defaultCipherList": "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA",
    "TLS1_VERSION": 769,
    "TLS1_1_VERSION": 770,
    "TLS1_2_VERSION": 771,
    "TLS1_3_VERSION": 772,
    "POINT_CONVERSION_COMPRESSED": 2,
    "POINT_CONVERSION_UNCOMPRESSED": 4,
    "POINT_CONVERSION_HYBRID": 6
  };

  // ---- o que o internal/promisify e o node_util_binding.rs entregam ao crypto ----

  function promisifyArgs(target, args) {
    Object.defineProperty(target, kCustomPromisifyArgsSymbol, { value: args, enumerable: false });
    return args;
  }

  function bunNormalizeEncoding(encoding) {
    if (encoding == null || encoding === "") return "utf8";
    switch (String(encoding).toLowerCase()) {
      case "utf8":
      case "utf-8":
        return "utf8";
      case "ucs2":
      case "ucs-2":
      case "utf16le":
      case "utf-16le":
        return "utf16le";
      case "latin1":
      case "binary":
        return "latin1";
      case "base64":
        return "base64";
      case "base64url":
        return "base64url";
      case "hex":
        return "hex";
      case "ascii":
        return "ascii";
    }
    return undefined;
  }

  // ---- codifica\u00E7\u00F5es e bytes ----

  const kEncodings = new Set(["utf8", "utf-8", "ucs2", "ucs-2", "utf16le", "utf-16le", "latin1", "binary", "ascii", "base64", "base64url", "hex", "buffer"]);
  const parseEncoding = value => (typeof value === "string" && kEncodings.has(value.toLowerCase()) ? value.toLowerCase() : undefined);
  // digest(encoding): valor falso \u00E9 buffer, o resto vira texto e o que n\u00E3o \u00E9 codifica\u00E7\u00E3o tamb\u00E9m \u00E9 buffer.
  const digestEncoding = value => (value ? (parseEncoding(String(value)) ?? "buffer") : "buffer");
  const toBuffer = bytes => globalThis.Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const encodeBytes = (bytes, encoding) => (encoding === "buffer" ? toBuffer(bytes.slice()) : toBuffer(bytes).toString(encoding));
  const isObject = value => value !== null && (typeof value === "object" || typeof value === "function");

  // Os bytes de update(dado, codifica\u00E7\u00E3o) (e do crypto.hash): texto na codifica\u00E7\u00E3o, ou uma vista de ArrayBuffer.
  function updateBytes(data, encoding) {
    if (typeof data === "string") {
      const parsed = parseEncoding(encoding) ?? "utf8";
      if (parsed === "hex" && data.length % 2 !== 0) {
        throw codes.INVALID_ARG_VALUE("encoding", encoding, "is invalid for data of length " + data.length);
      }
      return globalThis.Buffer.from(data, parsed === "buffer" ? "utf8" : parsed);
    }
    if (ArrayBuffer.isView(data)) return data;
    throw codes.INVALID_ARG_TYPE("data", "string or an instance of Buffer, TypedArray, or DataView", data);
  }

  const finalizedError = () => makeError("Error", "ERR_CRYPTO_HASH_FINALIZED", "Digest already called");
  const updateFailedError = () => makeError("Error", "ERR_CRYPTO_HASH_UPDATE_FAILED", "Hash update failed");

  // O erro que o BoringSSL deixa na fila quando o tamanho pedido n\u00E3o serve ao algoritmo (outputLength fora dos SHAKE).
  function notXofError() {
    const error = new Error("error:06000087:public key routines:OPENSSL_internal:NOT_XOF_OR_INVALID_LENGTH");
    error.library = "public key routines";
    error.function = "OPENSSL_internal";
    error.reason = "NOT_XOF_OR_INVALID_LENGTH";
    error.code = "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH";
    return error;
  }

  // ---- Hash e Hmac (os handles nativos do bun: JSHash.cpp e JSHmac.cpp) ----
  //
  // O handle do bun \u00E9 uma classe nativa cujas duas fun\u00E7\u00F5es aparecem no inspect como propriedades; aqui s\u00E3o
  // propriedades pr\u00F3prias de cada inst\u00E2ncia, fechadas sobre o estado.

  const hashStates = new WeakMap();

  class Hash {
    constructor(algorithm, options) {
      const original = hashStates.get(algorithm);
      let created;
      if (original !== undefined) {
        if (original.finalized) throw finalizedError();
        created = native.hashCopy(original.id);
        if (created === undefined) throw new Error("Digest copy error");
      } else {
        validators.validateString(algorithm, "algorithm");
        created = native.hashCreate(algorithm, false);
        if (created === undefined) throw new Error("Digest method not supported");
      }
      const [id, defaultLength, xof] = created;
      let length = defaultLength;
      try {
        if (isObject(options)) {
          const outputLength = options.outputLength;
          if (outputLength !== undefined) {
            validators.validateUint32(outputLength, "options.outputLength");
            if (outputLength !== length) {
              if (!xof) throw notXofError();
              length = outputLength;
            }
          }
        }
      } catch (error) {
        native.hashDrop(id);
        throw error;
      }
      const state = { id, length, finalized: false, digest: undefined };
      hashStates.set(this, state);
      this.update = function update(wrapper, data, encoding) {
        if (state.finalized) throw finalizedError();
        const bytes = updateBytes(data, encoding);
        if (state.digest !== undefined || !native.hashUpdate(state.id, bytes)) throw updateFailedError();
        return wrapper;
      };
      this.digest = function digest(encodingValue, setFinalized) {
        if (state.finalized) throw finalizedError();
        const encoding = digestEncoding(encodingValue);
        const finalized = typeof setFinalized === "boolean" ? setFinalized : true;
        if (state.digest === undefined) {
          if (state.length > 0) {
            state.digest = native.hashDigest(state.id, state.length);
          } else {
            native.hashDrop(state.id);
            state.digest = new Uint8Array(0);
          }
        }
        state.finalized = finalized;
        return encodeBytes(state.digest, encoding);
      };
    }
  }

  // A chave de um Hmac: texto (na codifica\u00E7\u00E3o de options.encoding, utf8 se n\u00E3o a conhece) ou bytes.
  function secretKeyBytes(key, encoding) {
    if (typeof key === "string") return globalThis.Buffer.from(key, parseEncoding(encoding) ?? "utf8");
    if (isAnyArrayBuffer(key) || ArrayBuffer.isView(key)) return key;
    throw codes.INVALID_ARG_TYPE("key", ["string", "ArrayBuffer", "Buffer", "TypedArray", "DataView", "KeyObject", "CryptoKey"], key);
  }

  class Hmac {
    constructor(algorithm, key, options) {
      validators.validateString(algorithm, "hmac");
      let encoding;
      if (isObject(options)) {
        encoding = options.encoding;
        if (encoding != null) validators.validateString(encoding, "options.encoding");
      }
      const secret = secretKeyBytes(key, encoding);
      const id = native.hmacCreate(algorithm, secret);
      if (id < 0) throw makeError("TypeError", "ERR_CRYPTO_INVALID_DIGEST", "Invalid digest: " + algorithm);
      const state = { id, finalized: false };
      this.update = function update(wrapper, data, encoding) {
        if (state.finalized) throw finalizedError();
        const bytes = updateBytes(data, encoding);
        if (!native.hashUpdate(state.id, bytes)) throw updateFailedError();
        return wrapper;
      };
      this.digest = function digest(encodingValue) {
        if (state.finalized) {
          // Um Hmac j\u00E1 terminado devolve vazio (o _flush do stream chama o digest antes do programa).
          const finishedEncoding = parseEncoding(encodingValue);
          return finishedEncoding !== undefined && finishedEncoding !== "buffer" ? "" : globalThis.Buffer.alloc(0);
        }
        const encoding = digestEncoding(encodingValue);
        const bytes = native.hashDigest(state.id, 0);
        state.finalized = true;
        return encodeBytes(bytes, encoding);
      };
    }
  }

  // crypto.hash(algoritmo, entrada, codifica\u00E7\u00E3o) (o CryptoHasher.hash do bun): s\u00F3 a tabela pr\u00F3pria do bun, mensagens medidas.
  function oneShotHash(algorithm, input, outputEncoding) {
    if (typeof algorithm !== "string") throw makeError("TypeError", "ERR_INVALID_ARG_TYPE", "Expected string");
    let bytes;
    if (typeof input === "string") bytes = globalThis.Buffer.from(input, "utf8");
    else if (ArrayBuffer.isView(input) || isAnyArrayBuffer(input)) bytes = input;
    else throw makeError("TypeError", "ERR_INVALID_ARG_TYPE", "expected blob, string or buffer");
    let encoding;
    if (typeof outputEncoding === "string") {
      encoding = parseEncoding(outputEncoding);
      if (encoding === undefined) throw makeError("TypeError", "ERR_INVALID_ARG_VALUE", "Unknown encoding: " + outputEncoding);
    } else if (!ArrayBuffer.isView(outputEncoding) && !isAnyArrayBuffer(outputEncoding)) {
      throw makeError("TypeError", "ERR_INVALID_ARG_TYPE", "expected string or buffer");
    }
    const created = native.hashCreate(algorithm, true);
    if (created === undefined) throw makeError("TypeError", "ERR_INVALID_ARG_TYPE", 'Unsupported algorithm "' + algorithm + '"');
    const [id, length] = created;
    native.hashUpdate(id, bytes);
    const digest = native.hashDigest(id, length);
    if (encoding !== undefined) return encodeBytes(digest, encoding);
    const target = ArrayBuffer.isView(outputEncoding)
      ? new Uint8Array(outputEncoding.buffer, outputEncoding.byteOffset, outputEncoding.byteLength)
      : new Uint8Array(outputEncoding);
    target.set(digest.subarray(0, target.length));
    return outputEncoding;
  }

  // ---- bytes aleat\u00F3rios (node_crypto_binding.rs, m\u00F3dulo random) ----

  const kMaxLength = 2147483647;
  const kMaxRange = 0xffffffffffff;

  function assertOffset(offset, elementSize, length) {
    if (typeof offset !== "number") throw codes.INVALID_ARG_TYPE("offset", "number", offset);
    offset *= elementSize;
    const max = Math.min(length, kMaxLength);
    if (Number.isNaN(offset) || offset > max || offset < 0) throw codes.OUT_OF_RANGE("offset", ">= 0 && <= " + max, offset);
    return offset >>> 0;
  }

  function assertSize(size, elementSize, offset, length) {
    validators.validateNumber(size, "size");
    size *= elementSize;
    if (Number.isNaN(size) || size > kMaxLength || size < 0) throw codes.OUT_OF_RANGE("size", ">= 0 && <= " + kMaxLength, size);
    if (size + offset > length) throw codes.OUT_OF_RANGE("size + offset", "<= " + length, size + offset);
    return size >>> 0;
  }

  // A conclus\u00E3o de um job fora da thread do programa: depois dos microtasks e antes dos timers e do que o programa agendou depois.
  const queueJob = callback => {
    setImmediate(callback);
  };

  function randomBytes(size, callback) {
    size = assertSize(size, 1, 0, kMaxLength + 1);
    if (callback !== undefined) validators.validateFunction(callback, "callback");
    const buffer = toBuffer(native.randomBytes(size));
    if (callback === undefined) return buffer;
    queueJob(() => callback(null, buffer));
    return undefined;
  }

  // O alvo de randomFill: o ArrayBuffer ou a vista (offset e size em bytes).
  function fillRandom(buf, offset, size) {
    const target = ArrayBuffer.isView(buf) ? new Uint8Array(buf.buffer, buf.byteOffset + offset, size) : new Uint8Array(buf, offset, size);
    target.set(native.randomBytes(size));
  }

  const isBufferSource = buf => isAnyArrayBuffer(buf) || ArrayBuffer.isView(buf);
  const invalidBuffer = buf => codes.INVALID_ARG_TYPE("buf", "ArrayBuffer or ArrayBufferView", buf);

  function randomFillSync(buf, offset, size) {
    if (!isBufferSource(buf)) throw invalidBuffer(buf);
    const elementSize = buf.BYTES_PER_ELEMENT || 1;
    offset = assertOffset(offset === undefined ? 0 : offset, elementSize, buf.byteLength);
    size = size === undefined ? buf.byteLength - offset : assertSize(size, elementSize, offset, buf.byteLength);
    if (size === 0) return buf;
    fillRandom(buf, offset, size);
    return buf;
  }

  function randomFill(buf, offset, size, callback) {
    if (!isBufferSource(buf)) throw invalidBuffer(buf);
    const elementSize = buf.BYTES_PER_ELEMENT || 1;
    if (typeof offset === "function") {
      callback = offset;
      offset = assertOffset(0, elementSize, buf.byteLength);
      size = buf.length;
    } else if (typeof size === "function") {
      callback = size;
      offset = assertOffset(offset, elementSize, buf.byteLength);
      size = undefined;
    } else {
      validators.validateFunction(callback, "callback");
      offset = assertOffset(offset, elementSize, buf.byteLength);
    }
    size = size === undefined ? buf.byteLength - offset : assertSize(size, elementSize, offset, buf.byteLength);
    if (size === 0) {
      callback(null, buf);
      return undefined;
    }
    queueJob(() => {
      fillRandom(buf, offset, size);
      callback(null, buf);
    });
    return undefined;
  }

  // ERR_OUT_OF_RANGE do node separa com _ os inteiros de mais de 32 bits.
  function withSeparators(value) {
    const digits = String(Math.abs(value));
    let text = "";
    for (let index = 0; index < digits.length; index++) {
      if (index !== 0 && (digits.length - index) % 3 === 0) text += "_";
      text += digits[index];
    }
    return (value < 0 ? "-" : "") + text;
  }

  function randomInt(min, max) {
    let callback = arguments[2];
    let minSpecified = true;
    if (max === undefined || typeof max === "function") {
      callback = max;
      max = min;
      min = 0;
      minSpecified = false;
    }
    if (callback !== undefined) validators.validateFunction(callback, "callback");
    if (!Number.isSafeInteger(min)) throw codes.INVALID_ARG_TYPE("min", "a safe integer", min);
    if (!Number.isSafeInteger(max)) throw codes.INVALID_ARG_TYPE("max", "a safe integer", max);
    if (max <= min) throw codes.OUT_OF_RANGE("max", 'greater than the value of "min" (' + min + ")", max);
    if (max - min > kMaxRange) {
      const received = withSeparators(max - min);
      throw codes.OUT_OF_RANGE(minSpecified ? "max - min" : "max", "<= " + kMaxRange, received);
    }
    // Sorteio sem vi\u00E9s sobre 48 bits: descarta o que cai no resto final.
    const range = max - min;
    const limit = 2 ** 48 - (2 ** 48 % range);
    let drawn;
    do {
      drawn = 0;
      for (const byte of native.randomBytes(6)) drawn = drawn * 256 + byte;
    } while (drawn >= limit);
    const result = min + (drawn % range);
    if (callback === undefined) return result;
    process.nextTick(callback, undefined, result);
    return undefined;
  }

  function uuidOptions(options) {
    if (options === undefined) return;
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      throw makeError(
        "TypeError",
        "ERR_INVALID_ARG_TYPE",
        'The "options" property must be of type object, got ' + (Array.isArray(options) ? "array" : typeof options),
      );
    }
    const flag = options.disableEntropyCache;
    if (flag !== undefined && typeof flag !== "boolean") {
      throw makeError(
        "TypeError",
        "ERR_INVALID_ARG_TYPE",
        'The "options.disableEntropyCache" property must be of type boolean, got ' + typeof flag,
      );
    }
  }

  function formatUuid(bytes) {
    let text = "";
    for (let index = 0; index < 16; index++) {
      if (index === 4 || index === 6 || index === 8 || index === 10) text += "-";
      text += (bytes[index] + 256).toString(16).slice(1);
    }
    return text;
  }

  function randomUUID(options) {
    uuidOptions(options);
    const bytes = native.randomBytes(16);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    return formatUuid(bytes);
  }

  function randomUUIDv7(options) {
    uuidOptions(options);
    const bytes = native.randomBytes(16);
    let now = Math.max(Date.now(), 0);
    for (let index = 5; index >= 0; index--) {
      bytes[index] = now % 256;
      now = Math.floor(now / 256);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    return formatUuid(bytes);
  }

  // ---- listas e ajustes que o BoringSSL do bun responde ----

  const kHashes = ["md4", "md5", "ripemd160", "sha1", "sha224", "sha256", "sha384", "sha512", "sha512-224", "sha512-256", "sha3-224", "sha3-256", "sha3-384", "sha3-512"];
  const kCiphers = [
    "aes-128-cbc", "aes-128-cfb", "aes-128-ctr", "aes-128-ecb", "aes-128-gcm", "aes-128-ofb", "aes-192-cbc", "aes-192-ctr",
    "aes-192-ecb", "aes-192-gcm", "aes-192-ofb", "aes-256-cbc", "aes-256-cfb", "aes-256-ctr", "aes-256-ecb", "aes-256-gcm",
    "aes-256-ofb", "bf-cbc", "bf-cfb", "bf-ecb", "des-cbc", "des-ecb", "des-ede", "des-ede-cbc", "des-ede3", "des-ede3-cbc",
    "rc2-cbc", "rc4",
  ];
  const kCurves = ["secp224r1", "prime256v1", "secp384r1", "secp521r1"];

  function getHashes() {
    return kHashes.slice();
  }
  function getCiphers() {
    return kCiphers.slice();
  }
  function getCurves() {
    return kCurves.slice();
  }
  function secureHeapUsed() {
    return undefined;
  }
  function getFips() {
    return 0;
  }
  function setFips(_enabled) {
    return undefined;
  }
  function setEngine(_engine, _flags) {
    throw makeError("Error", "ERR_CRYPTO_CUSTOM_ENGINE_NOT_SUPPORTED", "Custom engines not supported by BoringSSL");
  }

  // ---- o que ainda n\u00E3o foi portado: o nome e o length do bun, o NotImplementedError do bun ----

  function unported(name, length, statics = [], prototype = undefined) {
    const stub = function () {
      throwNotImplemented("crypto." + name);
    };
    Object.defineProperty(stub, "name", { value: name, configurable: true });
    Object.defineProperty(stub, "length", { value: length, configurable: true });
    if (prototype !== undefined) Object.defineProperty(stub, "prototype", { value: prototype, writable: true });
    for (const member of statics) Object.defineProperty(stub, member, { value: unported(member, 0), writable: true, configurable: true });
    return stub;
  }

  const cppBinding = {
    getCurves,
    certVerifySpkac: unported("certVerifySpkac", 1),
    certExportPublicKey: unported("certExportPublicKey", 1),
    certExportChallenge: unported("certExportChallenge", 1),
    getCiphers,
    getCipherInfo: unported("getCipherInfo", 1),
    Sign: unported("Sign", 0),
    sign: unported("sign", 4),
    Verify: unported("Verify", 0),
    verify: unported("verify", 4),
    Hmac,
    Hash,
    ECDH: unported("ECDH", 2, ["convertKey"], { setPublicKey: unported("setPublicKey", 2) }),
    DiffieHellman: unported("DiffieHellman", 2),
    DiffieHellmanGroup: unported("DiffieHellmanGroup", 1),
    diffieHellman: unported("diffieHellman", 2),
    checkPrime: unported("checkPrime", 3),
    checkPrimeSync: unported("checkPrimeSync", 2),
    generatePrime: unported("generatePrime", 3),
    generatePrimeSync: unported("generatePrimeSync", 2),
    Cipher: unported("Cipher", 5),
    hkdf: unported("hkdf", 6),
    hkdfSync: unported("hkdfSync", 5),
    publicEncrypt: unported("publicEncrypt", 2),
    publicDecrypt: unported("publicDecrypt", 2),
    privateEncrypt: unported("privateEncrypt", 2),
    privateDecrypt: unported("privateDecrypt", 2),
    KeyObject: unported("KeyObject", 2, ["from"]),
    createSecretKey: unported("createSecretKey", 2),
    createPublicKey: unported("createPublicKey", 1),
    createPrivateKey: unported("createPrivateKey", 1),
    generateKey: unported("generateKey", 3),
    generateKeySync: unported("generateKeySync", 2),
    generateKeyPair: unported("generateKeyPair", 3),
    generateKeyPairSync: unported("generateKeyPairSync", 2),
    X509Certificate: unported("X509Certificate", 1),
  };

  const rustBinding = {
    pbkdf2: unported("pbkdf2", 6),
    pbkdf2Sync: unported("pbkdf2Sync", 5),
    timingSafeEqual: native.timingSafeEqual,
    randomInt,
    randomUUID,
    randomUUIDv7,
    randomBytes,
    randomFillSync,
    randomFill,
    secureHeapUsed,
    getFips,
    setFips,
    setEngine,
    getHashes,
    scrypt: unported("scrypt", 5),
    scryptSync: unported("scryptSync", 4),
    argon2: unported("argon2", 10),
    argon2Sync: unported("argon2Sync", 9),
  };

  const require = name => {
    if (name === "internal/util/deprecate") return host.utilInternal("deprecate");
    return host.requireBuiltin(name);
  };
  return (() => {
    const StringDecoder = host.requireBuiltin("node:string_decoder").StringDecoder;
    const LazyTransform = host.requireInternal("internal/streams/lazy_transform");
    const { defineCustomPromisifyArgs } = { defineCustomPromisifyArgs: promisifyArgs };
    const Writable = host.requireInternal("internal/streams/writable");
    const {
      getCurves,
      certVerifySpkac,
      certExportPublicKey,
      certExportChallenge,
      getCiphers,
      getCipherInfo,
      Sign: _Sign,
      sign,
      Verify: _Verify,
      verify,
      Hmac: _Hmac,
      Hash: _Hash,
      ECDH,
      DiffieHellman,
      DiffieHellmanGroup,
      diffieHellman,
      checkPrime,
      checkPrimeSync,
      generatePrime,
      generatePrimeSync,
      Cipher,
      hkdf,
      hkdfSync,
      publicEncrypt,
      publicDecrypt,
      privateEncrypt,
      privateDecrypt,
      KeyObject,
      createSecretKey,
      createPublicKey,
      createPrivateKey,
      generateKey,
      generateKeySync,
      generateKeyPair,
      generateKeyPairSync,
      X509Certificate
    } = cppBinding;
    const {
      pbkdf2,
      pbkdf2Sync,
      timingSafeEqual,
      randomInt,
      randomUUID,
      randomUUIDv7,
      randomBytes,
      randomFillSync,
      randomFill,
      secureHeapUsed,
      getFips,
      setFips,
      setEngine,
      getHashes,
      scrypt,
      scryptSync,
      argon2: _argon2,
      argon2Sync: _argon2Sync
    } = rustBinding;
    const normalizeEncoding = bunNormalizeEncoding;
    const {
      validateFunction,
      validateInteger,
      validateObject,
      validateOneOf,
      validateString,
      validateUint32
    } = validators;
    const { deprecate } = host.utilInternal("deprecate");
    const kHandle = Symbol("kHandle");
    function verifySpkac(spkac, encoding) {
      return certVerifySpkac(getArrayBufferOrView(spkac, "spkac", encoding));
    }
    function exportPublicKey(spkac, encoding) {
      return certExportPublicKey(getArrayBufferOrView(spkac, "spkac", encoding));
    }
    function exportChallenge(spkac, encoding) {
      return certExportChallenge(getArrayBufferOrView(spkac, "spkac", encoding));
    }
    function Certificate() {
      if (!new.target) {
        return new Certificate;
      }
      this.verifySpkac = verifySpkac;
      this.exportPublicKey = exportPublicKey;
      this.exportChallenge = exportChallenge;
    }
    Certificate.prototype = {};
    Certificate.verifySpkac = verifySpkac;
    Certificate.exportPublicKey = exportPublicKey;
    Certificate.exportChallenge = exportChallenge;
    var Buffer = globalThis.Buffer;
    const { isAnyArrayBuffer, isArrayBufferView } = host.requireBuiltin("node:util/types");
    function getArrayBufferOrView(buffer, name, encoding) {
      if (isAnyArrayBuffer(buffer))
        return buffer;
      if (typeof buffer === "string") {
        if (encoding === "buffer")
          encoding = "utf8";
        return Buffer.from(buffer, encoding);
      }
      if (!isArrayBufferView(buffer)) {
        throw codes.INVALID_ARG_TYPE(name, ["string", "ArrayBuffer", "Buffer", "TypedArray", "DataView"], buffer);
      }
      return buffer;
    }
    const crypto = globalThis.crypto;
    var crypto_exports = {};
    crypto_exports.getRandomValues = (value) => crypto.getRandomValues(value);
    crypto_exports.constants = cryptoConstants;
    crypto_exports.KeyObject = KeyObject;
    crypto_exports.generateKey = generateKey;
    crypto_exports.generateKeySync = generateKeySync;
    defineCustomPromisifyArgs(generateKeyPair, ["publicKey", "privateKey"]);
    crypto_exports.generateKeyPair = generateKeyPair;
    crypto_exports.generateKeyPairSync = generateKeyPairSync;
    crypto_exports.createSecretKey = createSecretKey;
    crypto_exports.createPublicKey = createPublicKey;
    crypto_exports.createPrivateKey = createPrivateKey;
    var webcrypto = crypto;
    var _subtle = webcrypto.subtle;
    crypto_exports.hash = function hash(algorithm, input, outputEncoding = "hex") {
      return oneShotHash(algorithm, input, outputEncoding);
    };
    crypto_exports.pbkdf2 = pbkdf2;
    crypto_exports.pbkdf2Sync = pbkdf2Sync;
    crypto_exports.hkdf = hkdf;
    crypto_exports.hkdfSync = hkdfSync;
    crypto_exports.getCurves = getCurves;
    crypto_exports.getCipherInfo = getCipherInfo;
    crypto_exports.timingSafeEqual = timingSafeEqual;
    crypto_exports.webcrypto = webcrypto;
    crypto_exports.subtle = _subtle;
    crypto_exports.X509Certificate = X509Certificate;
    crypto_exports.Certificate = Certificate;
    function Sign(algorithm, options) {
      if (!(this instanceof Sign)) {
        return new Sign(algorithm, options);
      }
      validateString(algorithm, "algorithm");
      this[kHandle] = new _Sign;
      this[kHandle].init(algorithm);
      Writable.apply(this, [options]);
    }
    toClass(Sign, "Sign", Writable);
    Object.assign(Sign.prototype, {
      _write: function(chunk, encoding, callback) {
        this.update(chunk, encoding);
        callback();
      },
      update: function(data, encoding) {
        return this[kHandle].update(this, data, encoding);
      },
      sign: function(options, encoding) {
        return this[kHandle].sign(options, encoding);
      }
    });
    crypto_exports.Sign = Sign;
    crypto_exports.sign = sign;
    function createSign(algorithm, options) {
      return new Sign(algorithm, options);
    }
    crypto_exports.createSign = createSign;
    function Verify(algorithm, options) {
      if (!(this instanceof Verify)) {
        return new Verify(algorithm, options);
      }
      validateString(algorithm, "algorithm");
      this[kHandle] = new _Verify;
      this[kHandle].init(algorithm);
      Writable.apply(this, [options]);
    }
    toClass(Verify, "Verify", Writable);
    Verify.prototype._write = Sign.prototype._write;
    Verify.prototype.update = Sign.prototype.update;
    Object.assign(Verify.prototype, {
      verify: function(options, signature, sigEncoding) {
        return this[kHandle].verify(options, signature, sigEncoding);
      }
    });
    crypto_exports.Verify = Verify;
    crypto_exports.verify = verify;
    function createVerify(algorithm, options) {
      return new Verify(algorithm, options);
    }
    crypto_exports.createVerify = createVerify;
    function Hash(algorithm, options) {
      if (!new.target) {
        return new Hash(algorithm, options);
      }
      const handle = new _Hash(algorithm, options);
      this[kHandle] = handle;
      LazyTransform.apply(this, [options]);
    }
    toClass(Hash, "Hash", LazyTransform);
    Object.assign(Hash.prototype, {
      copy: function(options) {
        return new Hash(this[kHandle], options);
      },
      _transform: function(chunk, encoding, callback) {
        this[kHandle].update(this, chunk, encoding);
        callback();
      },
      _flush: function(callback) {
        this.push(this[kHandle].digest(null, false));
        callback();
      },
      update: function(data, encoding) {
        return this[kHandle].update(this, data, encoding);
      },
      digest: function(outputEncoding) {
        return this[kHandle].digest(outputEncoding);
      }
    });
    crypto_exports.Hash = deprecate(Hash, "crypto.Hash constructor is deprecated.", "DEP0179");
    crypto_exports.createHash = function createHash(algorithm, options) {
      return new Hash(algorithm, options);
    };
    function Hmac(hmac, key, options) {
      if (!new.target) {
        return new Hmac(hmac, key, options);
      }
      const handle = new _Hmac(hmac, key, options);
      this[kHandle] = handle;
      LazyTransform.apply(this, [options]);
    }
    toClass(Hmac, "Hmac", LazyTransform);
    Object.assign(Hmac.prototype, {
      update: function(data, encoding) {
        return this[kHandle].update(this, data, encoding);
      },
      digest: function(outputEncoding) {
        return this[kHandle].digest(outputEncoding);
      },
      _transform: function(chunk, encoding, callback) {
        this[kHandle].update(this, chunk, encoding);
        callback();
      },
      _flush: function(callback) {
        this.push(this[kHandle].digest());
        callback();
      }
    });
    crypto_exports.Hmac = deprecate(Hmac, "crypto.Hmac constructor is deprecated.", "DEP0181");
    crypto_exports.createHmac = function createHmac(hmac, key, options) {
      return new Hmac(hmac, key, options);
    };
    crypto_exports.getHashes = getHashes;
    crypto_exports.randomInt = randomInt;
    crypto_exports.randomFill = randomFill;
    crypto_exports.randomFillSync = randomFillSync;
    crypto_exports.randomBytes = randomBytes;
    crypto_exports.randomUUID = randomUUID;
    crypto_exports.randomUUIDv7 = randomUUIDv7;
    const kArgon2Types = { __proto__: null, argon2d: 0, argon2i: 1, argon2id: 2 };
    function getArgon2BufferSource(buffer, name) {
      if (isAnyArrayBuffer(buffer))
        return buffer;
      if (typeof buffer === "string")
        return Buffer.from(buffer, "utf8");
      if (!isArrayBufferView(buffer)) {
        throw codes.INVALID_ARG_TYPE(name, ["string", "ArrayBuffer", "Buffer", "TypedArray", "DataView"], buffer);
      }
      return buffer;
    }
    function checkArgon2(algorithm, parameters) {
      validateString(algorithm, "algorithm");
      validateOneOf(algorithm, "algorithm", ["argon2d", "argon2i", "argon2id"]);
      const type = kArgon2Types[algorithm];
      validateObject(parameters, "parameters");
      const { parallelism, tagLength, memory, passes } = parameters;
      const MAX_POSITIVE_UINT_32 = 2 ** 32 - 1;
      const message = getArgon2BufferSource(parameters.message, "parameters.message");
      validateInteger(message.byteLength, "parameters.message.byteLength", 0, MAX_POSITIVE_UINT_32);
      const nonce = getArgon2BufferSource(parameters.nonce, "parameters.nonce");
      validateInteger(nonce.byteLength, "parameters.nonce.byteLength", 8, MAX_POSITIVE_UINT_32);
      validateInteger(parallelism, "parameters.parallelism", 1, 2 ** 24 - 1);
      validateInteger(tagLength, "parameters.tagLength", 4, MAX_POSITIVE_UINT_32);
      validateInteger(memory, "parameters.memory", 8 * parallelism, MAX_POSITIVE_UINT_32);
      validateUint32(passes, "parameters.passes", true);
      let secret = parameters.secret;
      if (secret === undefined) {
        secret = new Uint8Array(0);
      } else {
        secret = getArgon2BufferSource(secret, "parameters.secret");
        validateInteger(secret.byteLength, "parameters.secret.byteLength", 0, MAX_POSITIVE_UINT_32);
      }
      let associatedData = parameters.associatedData;
      if (associatedData === undefined) {
        associatedData = new Uint8Array(0);
      } else {
        associatedData = getArgon2BufferSource(associatedData, "parameters.associatedData");
        validateInteger(associatedData.byteLength, "parameters.associatedData.byteLength", 0, MAX_POSITIVE_UINT_32);
      }
      return { message, nonce, secret, associatedData, tagLength, passes, parallelism, memory, type };
    }
    crypto_exports.argon2 = function argon2(algorithm, parameters, callback) {
      parameters = checkArgon2(algorithm, parameters);
      validateFunction(callback, "callback");
      _argon2(parameters.message, parameters.nonce, parameters.parallelism, parameters.tagLength, parameters.memory, parameters.passes, parameters.secret, parameters.associatedData, parameters.type, (err, result) => {
        if (err !== undefined)
          return callback(err);
        callback(null, result);
      });
    };
    crypto_exports.argon2Sync = function argon2Sync(algorithm, parameters) {
      parameters = checkArgon2(algorithm, parameters);
      return _argon2Sync(parameters.message, parameters.nonce, parameters.parallelism, parameters.tagLength, parameters.memory, parameters.passes, parameters.secret, parameters.associatedData, parameters.type);
    };
    crypto_exports.checkPrime = checkPrime;
    crypto_exports.checkPrimeSync = checkPrimeSync;
    crypto_exports.generatePrime = generatePrime;
    crypto_exports.generatePrimeSync = generatePrimeSync;
    crypto_exports.secureHeapUsed = secureHeapUsed;
    crypto_exports.setEngine = setEngine;
    crypto_exports.getFips = getFips;
    crypto_exports.setFips = setFips;
    Object.defineProperty(crypto_exports, "fips", {
      __proto__: null,
      get: getFips,
      set: setFips
    });
    for (const rng of ["pseudoRandomBytes", "prng", "rng"]) {
      Object.defineProperty(crypto_exports, rng, {
        value: deprecate(randomBytes, `crypto.${rng} is deprecated.`, "DEP0115"),
        enumerable: false,
        configurable: true
      });
    }
    function createDiffieHellman(sizeOrKey, keyEncoding, generator, genEncoding) {
      return new DiffieHellman(sizeOrKey, keyEncoding, generator, genEncoding);
    }
    crypto_exports.DiffieHellmanGroup = DiffieHellmanGroup;
    crypto_exports.getDiffieHellman = crypto_exports.createDiffieHellmanGroup = DiffieHellmanGroup;
    crypto_exports.createDiffieHellman = createDiffieHellman;
    crypto_exports.DiffieHellman = DiffieHellman;
    crypto_exports.diffieHellman = diffieHellman;
    ECDH.prototype.setPublicKey = deprecate(ECDH.prototype.setPublicKey, "ecdh.setPublicKey() is deprecated.", "DEP0031");
    crypto_exports.ECDH = ECDH;
    crypto_exports.createECDH = function createECDH(curve) {
      return new ECDH(curve);
    };
    {
      let getDecoder = function(decoder, encoding) {
        const normalizedEncoding = normalizeEncoding(encoding);
        decoder ||= new StringDecoder(encoding);
        if (decoder.encoding !== normalizedEncoding) {
          if (normalizedEncoding === undefined) {
            throw codes.UNKNOWN_ENCODING(encoding);
          }
          throw codes.INTERNAL_ASSERTION("Cannot change encoding");
        }
        return decoder;
      }, Cipheriv2 = function(cipher, key, iv, options) {
        if (!new.target) {
          return new Cipheriv2(cipher, key, iv, options);
        }
        this[kHandle] = new Cipher(false, cipher, key, iv, options);
        LazyTransform.apply(this, [options]);
        this._decoder = null;
      }, Decipheriv2 = function(cipher, key, iv, options) {
        if (!new.target) {
          return new Decipheriv2(cipher, key, iv, options);
        }
        this[kHandle] = new Cipher(true, cipher, key, iv, options);
        LazyTransform.apply(this, [options]);
        this._decoder = null;
      };
      toClass(Cipheriv2, "Cipheriv", LazyTransform);
      Object.assign(Cipheriv2.prototype, {
        setAutoPadding: function(ap) {
          this[kHandle].setAutoPadding(ap);
          return this;
        },
        getAuthTag: function() {
          return this[kHandle].getAuthTag();
        },
        setAAD: function(aadbuf, options) {
          this[kHandle].setAAD(aadbuf, options);
          return this;
        },
        _transform: function(chunk, encoding, callback) {
          this.push(this[kHandle].update(chunk, encoding));
          callback();
        },
        _flush: function(callback) {
          try {
            this.push(this[kHandle].final());
          } catch (e) {
            callback(e);
            return;
          }
          callback();
        },
        update: function(data, inputEncoding, outputEncoding) {
          const res = this[kHandle].update(data, inputEncoding);
          if (outputEncoding && outputEncoding !== "buffer") {
            this._decoder = getDecoder(this._decoder, outputEncoding);
            return this._decoder.write(res);
          }
          return res;
        },
        final: function(outputEncoding) {
          const res = this[kHandle].final();
          if (outputEncoding && outputEncoding !== "buffer") {
            this._decoder = getDecoder(this._decoder, outputEncoding);
            return this._decoder.end(res);
          }
          return res;
        }
      });
      toClass(Decipheriv2, "Decipheriv", LazyTransform);
      Object.assign(Decipheriv2.prototype, {
        setAutoPadding: Cipheriv2.prototype.setAutoPadding,
        setAuthTag: function(tagbuf, encoding) {
          this[kHandle].setAuthTag(tagbuf, encoding);
          return this;
        },
        setAAD: Cipheriv2.prototype.setAAD,
        _transform: Cipheriv2.prototype._transform,
        _flush: Cipheriv2.prototype._flush,
        update: Cipheriv2.prototype.update,
        final: Cipheriv2.prototype.final
      });
      crypto_exports.Cipheriv = Cipheriv2;
      crypto_exports.Decipheriv = Decipheriv2;
      crypto_exports.createCipheriv = function createCipheriv(cipher, key, iv, options) {
        return new Cipheriv2(cipher, key, iv, options);
      };
      crypto_exports.createDecipheriv = function createDecipheriv(cipher, key, iv, options) {
        return new Decipheriv2(cipher, key, iv, options);
      };
      crypto_exports.getCiphers = getCiphers;
    }
    crypto_exports.scrypt = scrypt;
    crypto_exports.scryptSync = scryptSync;
    crypto_exports.publicEncrypt = publicEncrypt;
    crypto_exports.publicDecrypt = publicDecrypt;
    crypto_exports.privateEncrypt = privateEncrypt;
    crypto_exports.privateDecrypt = privateDecrypt;
    return crypto_exports;

  })();
})
