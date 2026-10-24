// GERADO por `scripts/gen-node-util.js` a partir do `util.ts` e dos internos que ele usa, do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";

  const { codes } = host.internal;
  // `$isArray` e `$min`: os intrínsecos guardados antes de qualquer código do usuário.
  const ArrayIsArrayIntrinsic = Array.isArray;
  const MathMinIntrinsic = Math.min;
  // `$isObject`: objeto ou função. O nome não é `isObject` porque o `util.ts` declara a função `isObject` do `util`.
  const isObjectIntrinsic = value => (typeof value === "object" && value !== null) || typeof value === "function";
const primordials = (() => {
const ObjectSetPrototypeOf = Object.setPrototypeOf;
const ObjectFreeze = Object.freeze;
const createSafeIterator = (factory, next_) => {

  class SafeIterator {
    constructor(iterable) {
      this._iterator = factory(iterable);
    }
    next() {
      return next_(this._iterator);
    }
    [Symbol.iterator]() {
      return this;
    }
  }
  ObjectSetPrototypeOf(SafeIterator.prototype, null);
  ObjectFreeze(SafeIterator.prototype);
  ObjectFreeze(SafeIterator);
  return SafeIterator;
};
const FunctionPrototypeCall = Function.prototype.call;
const ObjectPrototypeHasOwnProperty = FunctionPrototypeCall.bind(Object.prototype.hasOwnProperty);
function getGetter(cls, getter) {
  return FunctionPrototypeCall.bind(cls.prototype.__lookupGetter__(getter));
}
function uncurryThis(func) {
  return FunctionPrototypeCall.bind(func);
}
const copyProps = (src, dest) => {
  const keys = Reflect.ownKeys(src);
  for (let i = 0;i < keys.length; i++) {
    const key = keys[i];
    if (!ObjectPrototypeHasOwnProperty(dest, key)) {
      Reflect.defineProperty(dest, key, Reflect.getOwnPropertyDescriptor(src, key));
    }
  }
};
const SymbolIterator = Symbol.iterator;
const makeSafe = (unsafe, safe) => {
  const unsafePrototype = unsafe.prototype;
  const safePrototype = safe.prototype;
  if (SymbolIterator in unsafePrototype) {
    const dummy = new unsafe;
    let next;
    const keys = Reflect.ownKeys(unsafePrototype);
    for (let i = 0;i < keys.length; i++) {
      const key = keys[i];
      if (ObjectPrototypeHasOwnProperty(safePrototype, key))
        continue;
      const desc = Reflect.getOwnPropertyDescriptor(unsafePrototype, key);
      if (typeof desc.value === "function" && desc.value.length === 0) {
        const called = desc.value.call(dummy) || {};
        if (SymbolIterator in (typeof called === "object" ? called : {})) {
          const createIterator = uncurryThis(desc.value);
          next ??= uncurryThis(createIterator(dummy).next);
          const SafeIterator = createSafeIterator(createIterator, next);
          desc.value = function() {
            return new SafeIterator(this);
          };
        }
      }
      Reflect.defineProperty(safePrototype, key, desc);
    }
  } else
    copyProps(unsafePrototype, safePrototype);
  copyProps(unsafe, safe);
  Object.setPrototypeOf(safePrototype, null);
  Object.freeze(safePrototype);
  Object.freeze(safe);
  return safe;
};
const StringIterator = uncurryThis(String.prototype[Symbol.iterator]);
const StringIteratorPrototype = Reflect.getPrototypeOf(StringIterator(""));
const ArrayPrototypeSymbolIterator = uncurryThis(Array.prototype[Symbol.iterator]);
const ArrayIteratorPrototypeNext = uncurryThis(Array.prototype[Symbol.iterator]().next);
const SafeArrayIterator = createSafeIterator(ArrayPrototypeSymbolIterator, ArrayIteratorPrototypeNext);
const PromisePrototypeThen = Promise.prototype.then;
const PromiseResolve = Promise.resolve.bind(Promise);
const safePromiseAllCollect = (promises, mapFn, returnVal) => new Promise((resolve, reject) => {
  const { length } = promises;
  if (length === 0)
    resolve(returnVal ?? undefined);
  let pendingPromises = length;
  for (let i = 0;i < length; i++) {
    const promise = mapFn != null ? mapFn(promises[i], i) : promises[i];
    PromisePrototypeThen.call(PromiseResolve(promise), (result) => {
      if (returnVal !== null)
        returnVal[i] = result;
      if (--pendingPromises === 0)
        resolve(returnVal ?? undefined);
    }, reject);
  }
});
const SafePromiseAllReturnVoid = (promises, mapFn) => safePromiseAllCollect(promises, mapFn, null);
const SafePromiseAllReturnArrayLike = (promises, mapFn) => {
  const returnVal = Array(promises.length);
  ObjectSetPrototypeOf(returnVal, null);
  return safePromiseAllCollect(promises, mapFn, returnVal);
};
return {
  SafeArrayIterator,
  MapPrototypeGetSize: getGetter(Map, "size"),
  SafeStringIterator: createSafeIterator(StringIterator, uncurryThis(StringIteratorPrototype.next)),
  SafeMap: makeSafe(Map, class SafeMap extends Map {
    constructor(i) {
      super(i);
    }
  }),
  SafePromiseAllReturnArrayLike,
  SafePromiseAllReturnVoid,
  SafeSet: makeSafe(Set, class SafeSet extends Set {
    constructor(i) {
      super(i);
    }
  }),
  SafeWeakMap: makeSafe(WeakMap, class SafeWeakMap extends WeakMap {
    constructor(i) {
      super(i);
    }
  }),
  SetPrototypeGetSize: getGetter(Set, "size"),
  TypedArrayPrototypeGetBuffer: getGetter(Uint8Array, "buffer"),
  TypedArrayPrototypeGetByteLength: getGetter(Uint8Array, "byteLength"),
  TypedArrayPrototypeGetByteOffset: getGetter(Uint8Array, "byteOffset"),
  TypedArrayPrototypeGetLength: getGetter(Uint8Array, "length"),
  TypedArrayPrototypeGetSymbolToStringTag: getGetter(Uint8Array, Symbol.toStringTag),
  Uint8ClampedArray,
  Uint8Array,
  Uint16Array,
  Uint32Array,
  Int8Array,
  Int16Array,
  Int32Array,
  Float16Array,
  Float32Array,
  Float64Array,
  BigUint64Array,
  BigInt64Array,
  uncurryThis
};

})();
const ttyModule = (() => {
let OSRelease;
const COLORS_2 = 1;
const COLORS_16 = 4;
const COLORS_256 = 8;
const COLORS_16m = 24;
const TERM_ENVS = {
  eterm: 4,
  cons25: 4,
  console: 4,
  cygwin: 4,
  dtterm: 4,
  gnome: 4,
  hurd: 4,
  jfbterm: 4,
  konsole: 4,
  kterm: 4,
  mlterm: 4,
  mosh: 24,
  putty: 4,
  st: 4,
  "rxvt-unicode-24bit": 24,
  terminator: 24,
  "xterm-kitty": 24
};
const CI_ENVS = {
  APPVEYOR: 8,
  BUILDKITE: 8,
  CIRCLECI: 24,
  DRONE: 8,
  GITEA_ACTIONS: 24,
  GITHUB_ACTIONS: 24,
  GITLAB_CI: 8,
  TRAVIS: 8
};
const TERM_ENVS_REG_EXP = [
  /ansi/,
  /color/,
  /linux/,
  /direct/,
  /^con[0-9]*x[0-9]/,
  /^rxvt/,
  /^screen/,
  /^xterm/,
  /^vt100/,
  /^vt220/
];
let warned = false;
function warnOnDeactivatedColors(env) {
  if (warned)
    return;
  let name = "";
  if (env.NODE_DISABLE_COLORS !== undefined && env.NODE_DISABLE_COLORS !== "")
    name = "NODE_DISABLE_COLORS";
  if (env.NO_COLOR !== undefined && env.NO_COLOR !== "") {
    if (name !== "") {
      name += "' and '";
    }
    name += "NO_COLOR";
  }
  if (name !== "") {
    process.emitWarning(`The '${name}' env is ignored due to the 'FORCE_COLOR' env being set.`, "Warning");
    warned = true;
  }
}
function getColorDepth(env) {
  const FORCE_COLOR = env.FORCE_COLOR;
  if (FORCE_COLOR !== undefined) {
    switch (FORCE_COLOR) {
      case "":
      case "1":
      case "true":
        warnOnDeactivatedColors(env);
        return 4;
      case "2":
        warnOnDeactivatedColors(env);
        return 8;
      case "3":
        warnOnDeactivatedColors(env);
        return 24;
      default:
        return 1;
    }
  }
  if (env.NODE_DISABLE_COLORS !== undefined && env.NODE_DISABLE_COLORS !== "" || env.NO_COLOR !== undefined && env.NO_COLOR !== "" || env.TERM === "dumb") {
    return 1;
  }
  if (process.platform === "win32") {
    if (OSRelease === undefined) {
      const { release } = host.requireBuiltin("node:os");
      OSRelease = release().split(".");
    }
    if (+OSRelease[0] >= 10) {
      const build = +OSRelease[2];
      if (build >= 14931)
        return 24;
      if (build >= 10586)
        return 8;
    }
    return 4;
  }
  if (env.TMUX) {
    return 24;
  }
  if ("TF_BUILD" in env && "AGENT_NAME" in env) {
    return 4;
  }
  if ("CI" in env) {
    for (const name in CI_ENVS) {
      if (name in env) {
        return CI_ENVS[name];
      }
    }
    if (env.CI_NAME === "codeship") {
      return 8;
    }
    return 1;
  }
  if ("TEAMCITY_VERSION" in env) {
    return /^(9\.(0*[1-9]\d*)\.|\d{2,}\.)/.test(env.TEAMCITY_VERSION) ? 4 : 1;
  }
  switch (env.TERM_PROGRAM) {
    case "iTerm.app":
      if (!env.TERM_PROGRAM_VERSION || /^[0-2]\./.test(env.TERM_PROGRAM_VERSION)) {
        return 8;
      }
      return 24;
    case "HyperTerm":
    case "ghostty":
    case "WezTerm":
    case "MacTerm":
      return 24;
    case "Apple_Terminal":
      return 8;
  }
  const COLORTERM = env.COLORTERM;
  if (COLORTERM === "truecolor" || COLORTERM === "24bit") {
    return 24;
  }
  const TERM = env.TERM;
  if (TERM) {
    if (/truecolor/.test(TERM)) {
      return 24;
    }
    if (TERM.startsWith("xterm-256")) {
      return 8;
    }
    const termEnv = TERM.toLowerCase();
    if (TERM_ENVS[termEnv]) {
      return TERM_ENVS[termEnv];
    }
    if (TERM_ENVS_REG_EXP.some((term) => term.test(termEnv))) {
      return 4;
    }
  }
  if (env.COLORTERM) {
    return 4;
  }
  return 1;
}
return { getColorDepth };

})();
const colorsModule = (() => {
let exports = {
  blue: "",
  green: "",
  white: "",
  red: "",
  hasColors: false,
  shouldColorize(stream) {
    if (process.env.FORCE_COLOR !== undefined) {
      return ttyModule.getColorDepth(process.env) > 2;
    }
    return stream?.isTTY && (typeof stream.getColorDepth === "function" ? stream.getColorDepth() > 2 : true);
  },
  refresh() {
    if (exports.shouldColorize(process.stderr)) {
      exports.blue = "\x1B[34m";
      exports.green = "\x1B[32m";
      exports.white = "\x1B[39m";
      exports.red = "\x1B[31m";
      exports.hasColors = true;
    } else {
      exports.blue = "";
      exports.green = "";
      exports.white = "";
      exports.red = "";
      exports.hasColors = false;
    }
  }
};
exports.refresh();
return exports;

})();
const inspectModule = (() => {
let BufferModule;
const { uncurryThis } = primordials;
const {
  MapPrototypeGetSize,
  SafeMap,
  SafeSet,
  SetPrototypeGetSize,
  TypedArrayPrototypeGetLength,
  TypedArrayPrototypeGetSymbolToStringTag
} = primordials;
const ArrayFrom = Array.from;
const ArrayPrototypeFilter = uncurryThis(Array.prototype.filter);
const ArrayPrototypeFlat = uncurryThis(Array.prototype.flat);
const ArrayPrototypeForEach = uncurryThis(Array.prototype.forEach);
const ArrayPrototypeIncludes = uncurryThis(Array.prototype.includes);
const ArrayPrototypeIndexOf = uncurryThis(Array.prototype.indexOf);
const ArrayPrototypeJoin = uncurryThis(Array.prototype.join);
const ArrayPrototypeMap = uncurryThis(Array.prototype.map);
const ArrayPrototypePop = uncurryThis(Array.prototype.pop);
const ArrayPrototypePush = Array.prototype.push;
const ArrayPrototypeSlice = uncurryThis(Array.prototype.slice);
const ArrayPrototypeSplice = uncurryThis(Array.prototype.splice);
const ArrayPrototypeSort = uncurryThis(Array.prototype.sort);
const ArrayPrototypeUnshift = uncurryThis(Array.prototype.unshift);
const BigIntPrototypeValueOf = uncurryThis(BigInt.prototype.valueOf);
const BooleanPrototypeValueOf = uncurryThis(Boolean.prototype.valueOf);
const DatePrototypeGetTime = uncurryThis(Date.prototype.getTime);
const DatePrototypeToISOString = uncurryThis(Date.prototype.toISOString);
const DatePrototypeToString = uncurryThis(Date.prototype.toString);
const ErrorPrototypeToString = uncurryThis(Error.prototype.toString);
const FunctionPrototypeBind = uncurryThis(Function.prototype.bind);
const FunctionPrototypeToString = uncurryThis(Function.prototype.toString);
const JSONStringify = JSON.stringify;
const MapPrototypeEntries = uncurryThis(Map.prototype.entries);
const MapPrototypeValues = uncurryThis(Map.prototype.values);
const MapPrototypeKeys = uncurryThis(Map.prototype.keys);
const MathFloor = Math.floor;
const MathMax = Math.max;
const MathRound = Math.round;
const MathSqrt = Math.sqrt;
const MathTrunc = Math.trunc;
const NumberIsFinite = Number.isFinite;
const NumberIsNaN = Number.isNaN;
const NumberParseFloat = Number.parseFloat;
const NumberParseInt = Number.parseInt;
const NumberPrototypeToString = uncurryThis(Number.prototype.toString);
const NumberPrototypeValueOf = uncurryThis(Number.prototype.valueOf);
const ObjectAssign = Object.assign;
const ObjectDefineProperty = Object.defineProperty;
const ObjectGetOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
const ObjectGetOwnPropertyNames = Object.getOwnPropertyNames;
const ObjectGetOwnPropertySymbols = Object.getOwnPropertySymbols;
const ObjectGetPrototypeOf = Object.getPrototypeOf;
const ObjectIs = Object.is;
const ObjectKeys = Object.keys;
const ObjectPrototypeHasOwnProperty = uncurryThis(Object.prototype.hasOwnProperty);
const ObjectPrototypePropertyIsEnumerable = uncurryThis(Object.prototype.propertyIsEnumerable);
const ObjectPrototypeToString = uncurryThis(Object.prototype.toString);
const ObjectSeal = Object.seal;
const ObjectSetPrototypeOf = Object.setPrototypeOf;
const ReflectOwnKeys = Reflect.ownKeys;
const RegExpPrototypeExec = uncurryThis(RegExp.prototype.exec);
const RegExpPrototypeSymbolReplace = uncurryThis(RegExp.prototype[Symbol.replace]);
const RegExpPrototypeSymbolSplit = uncurryThis(RegExp.prototype[Symbol.split]);
const RegExpPrototypeToString = uncurryThis(RegExp.prototype.toString);
const SetPrototypeEntries = uncurryThis(Set.prototype.entries);
const SetPrototypeValues = uncurryThis(Set.prototype.values);
const StringPrototypeCharCodeAt = uncurryThis(String.prototype.charCodeAt);
const StringPrototypeIncludes = uncurryThis(String.prototype.includes);
const StringPrototypeIndexOf = uncurryThis(String.prototype.indexOf);
const StringPrototypeLastIndexOf = uncurryThis(String.prototype.lastIndexOf);
const StringPrototypeMatch = uncurryThis(String.prototype.match);
const StringPrototypeNormalize = uncurryThis(String.prototype.normalize);
const StringPrototypePadEnd = uncurryThis(String.prototype.padEnd);
const StringPrototypePadStart = uncurryThis(String.prototype.padStart);
const StringPrototypeRepeat = uncurryThis(String.prototype.repeat);
const StringPrototypeReplaceAll = uncurryThis(String.prototype.replaceAll);
const StringPrototypeSlice = uncurryThis(String.prototype.slice);
const StringPrototypeSplit = uncurryThis(String.prototype.split);
const StringPrototypeEndsWith = uncurryThis(String.prototype.endsWith);
const StringPrototypeStartsWith = uncurryThis(String.prototype.startsWith);
const StringPrototypeToLowerCase = uncurryThis(String.prototype.toLowerCase);
const StringPrototypeTrim = uncurryThis(String.prototype.trim);
const StringPrototypeValueOf = uncurryThis(String.prototype.valueOf);
const SymbolPrototypeToString = uncurryThis(Symbol.prototype.toString);
const SymbolPrototypeValueOf = uncurryThis(Symbol.prototype.valueOf);
const SymbolIterator = Symbol.iterator;
const SymbolToStringTag = Symbol.toStringTag;
const customInspectSymbol = Symbol.for("nodejs.util.inspect.custom");
const kPending = Symbol("kPending");
const kFulfilled = Symbol("kFulfilled");
const kRejected = Symbol("kRejected");
const ALL_PROPERTIES = 0;
const ONLY_ENUMERABLE = 2;
const extractedSplitNewLinesFastPathStringsOnly = host.splitNewLines;
const isAsyncFunction = (v) => typeof v === "function" && StringPrototypeStartsWith(FunctionPrototypeToString(v), "async");
const isGeneratorFunction = (v) => typeof v === "function" && StringPrototypeMatch(FunctionPrototypeToString(v), /^(async\s+)?function *\*/);
const {
  isAnyArrayBuffer,
  isArrayBuffer,
  isArgumentsObject,
  isBigIntObject,
  isBooleanObject,
  isBoxedPrimitive,
  isDataView,
  isExternal,
  isMap,
  isMapIterator,
  isModuleNamespaceObject,
  isNativeError,
  isNumberObject,
  isPromise,
  isSet,
  isSetIterator,
  isStringObject,
  isWeakMap,
  isWeakSet,
  isRegExp,
  isDate,
  isTypedArray
} = host.requireBuiltin("node:util/types");
//! The native versions of the commented out functions are currently buggy, so we use the polyfills above for now.

class AssertionError extends Error {
  constructor(message, isForced = false) {
    super(message);
    this.name = "AssertionError";
    this.code = "ERR_ASSERTION";
    this.operator = "==";
    this.generatedMessage = !isForced;
    this.actual = isForced && undefined;
    this.expected = !isForced || undefined;
  }
}
function assert(p, message) {
  if (!p)
    throw new AssertionError(message);
}
const { validateObject, kValidateObjectAllowArray } = host.internal.validators;
const SymbolToPrimitive = Symbol.toPrimitive;
const builtInObjects = new SafeSet(ArrayPrototypeFilter(ObjectGetOwnPropertyNames(globalThis), (e) => RegExpPrototypeExec(/^[A-Z][a-zA-Z0-9]+$/, e) !== null));
const isUndetectableObject = (v) => typeof v === "undefined" && v !== undefined;
const ERROR_STACK_OVERFLOW_MSG = "Maximum call stack size exceeded.";
const inspectDefaultOptions = ObjectSeal({
  showHidden: false,
  depth: 2,
  colors: false,
  customInspect: true,
  showProxy: false,
  maxArrayLength: 100,
  maxStringLength: 1e4,
  breakLength: 80,
  compact: 3,
  sorted: false,
  getters: false,
  numericSeparator: false
});
const inspectReplDefaults = ObjectSeal({
  ...inspectDefaultOptions,
  colors: Bun.enableANSIColors,
  showProxy: true
});
const kObjectType = 0;
const kArrayType = 1;
const kArrayExtrasType = 2;
const strEscapeSequencesRegExp = /[\x00-\x1f\x27\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;
const strEscapeSequencesReplacer = /[\x00-\x1f\x27\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/g;
const strEscapeSequencesRegExpSingle = /[\x00-\x1f\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;
const strEscapeSequencesReplacerSingle = /[\x00-\x1f\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/g;
const extractedNewLineRe = /(?<=\n)/;
const extractedSplitNewLinesSlow = (value) => RegExpPrototypeSymbolSplit(extractedNewLineRe, value);
const extractedSplitNewLines = (value) => {
  if (typeof value === "string") {
    return extractedSplitNewLinesFastPathStringsOnly(value) || extractedSplitNewLinesSlow(value);
  }
  return extractedSplitNewLinesSlow(value);
};
const keyStrRegExp = /^[a-zA-Z_][a-zA-Z_0-9]*$/;
const numberRegExp = /^(0|[1-9][0-9]*)$/;
const coreModuleRegExp = /^ {4}at (?:[^/\\(]+ \(|)node:(.+):\d+:\d+\)?$/;
const nodeModulesRegExp = /[/\\]node_modules[/\\](.+?)(?=[/\\])/g;
const classRegExp = /^(\s+[^(]*?)\s*{/;
const stripCommentsRegExp = /(\/\/.*?\n)|(\/\*(.|\n)*?\*\/)/g;
const kMinLineLength = 16;
const kWeak = 0;
const kIterator = 1;
const kMapEntries = 2;
const meta = [
  "\\x00",
  "\\x01",
  "\\x02",
  "\\x03",
  "\\x04",
  "\\x05",
  "\\x06",
  "\\x07",
  "\\b",
  "\\t",
  "\\n",
  "\\x0B",
  "\\f",
  "\\r",
  "\\x0E",
  "\\x0F",
  "\\x10",
  "\\x11",
  "\\x12",
  "\\x13",
  "\\x14",
  "\\x15",
  "\\x16",
  "\\x17",
  "\\x18",
  "\\x19",
  "\\x1A",
  "\\x1B",
  "\\x1C",
  "\\x1D",
  "\\x1E",
  "\\x1F",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "\\'",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "\\\\",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "",
  "\\x7F",
  "\\x80",
  "\\x81",
  "\\x82",
  "\\x83",
  "\\x84",
  "\\x85",
  "\\x86",
  "\\x87",
  "\\x88",
  "\\x89",
  "\\x8A",
  "\\x8B",
  "\\x8C",
  "\\x8D",
  "\\x8E",
  "\\x8F",
  "\\x90",
  "\\x91",
  "\\x92",
  "\\x93",
  "\\x94",
  "\\x95",
  "\\x96",
  "\\x97",
  "\\x98",
  "\\x99",
  "\\x9A",
  "\\x9B",
  "\\x9C",
  "\\x9D",
  "\\x9E",
  "\\x9F"
];
function getUserOptions(ctx, isCrossContext) {
  const ret = {
    stylize: ctx.stylize,
    showHidden: ctx.showHidden,
    depth: ctx.depth,
    colors: ctx.colors,
    customInspect: ctx.customInspect,
    showProxy: ctx.showProxy,
    maxArrayLength: ctx.maxArrayLength,
    maxStringLength: ctx.maxStringLength,
    breakLength: ctx.breakLength,
    compact: ctx.compact,
    sorted: ctx.sorted,
    getters: ctx.getters,
    numericSeparator: ctx.numericSeparator,
    ...ctx.userOptions
  };
  if (isCrossContext) {
    ObjectSetPrototypeOf(ret, null);
    for (const key of ObjectKeys(ret)) {
      if ((typeof ret[key] === "object" || typeof ret[key] === "function") && ret[key] !== null) {
        delete ret[key];
      }
    }
    ret.stylize = ObjectSetPrototypeOf((value, flavour) => {
      let stylized;
      try {
        stylized = `${ctx.stylize(value, flavour)}`;
      } catch {}
      if (typeof stylized !== "string")
        return value;
      return stylized;
    }, null);
  }
  return ret;
}
function inspect(value, opts) {
  const ctx = {
    budget: {},
    indentationLvl: 0,
    seen: [],
    currentDepth: 0,
    stylize: stylizeNoColor,
    showHidden: inspectDefaultOptions.showHidden,
    depth: inspectDefaultOptions.depth,
    colors: inspectDefaultOptions.colors,
    customInspect: inspectDefaultOptions.customInspect,
    showProxy: inspectDefaultOptions.showProxy,
    maxArrayLength: inspectDefaultOptions.maxArrayLength,
    maxStringLength: inspectDefaultOptions.maxStringLength,
    breakLength: inspectDefaultOptions.breakLength,
    compact: inspectDefaultOptions.compact,
    sorted: inspectDefaultOptions.sorted,
    getters: inspectDefaultOptions.getters,
    numericSeparator: inspectDefaultOptions.numericSeparator
  };
  const argc = arguments.length;
  if (argc > 1) {
    if (argc > 2) {
      if (arguments[2] !== undefined) {
        ctx.depth = arguments[2];
      }
      if (argc > 3 && arguments[3] !== undefined) {
        ctx.colors = arguments[3];
      }
    }
    if (typeof opts === "boolean") {
      ctx.showHidden = opts;
    } else if (opts) {
      const optKeys = ObjectKeys(opts);
      for (let i = 0;i < optKeys.length; ++i) {
        const key = optKeys[i];
        if (ObjectPrototypeHasOwnProperty(inspectDefaultOptions, key) || key === "stylize") {
          ctx[key] = opts[key];
        } else if (ctx.userOptions === undefined) {
          ctx.userOptions = opts;
        }
      }
    }
  }
  if (ctx.colors)
    ctx.stylize = stylizeWithColor;
  if (ctx.maxArrayLength === null)
    ctx.maxArrayLength = 1 / 0;
  if (ctx.maxStringLength === null)
    ctx.maxStringLength = 1 / 0;
  return formatValue(ctx, value, 0);
}
inspect.custom = customInspectSymbol;
ObjectDefineProperty(inspect, "defaultOptions", {
  __proto__: null,
  get() {
    return inspectDefaultOptions;
  },
  set(options) {
    validateObject(options, "options");
    return ObjectAssign(inspectDefaultOptions, options);
  }
});
ObjectDefineProperty(inspect, "replDefaults", {
  __proto__: null,
  get() {
    return inspectReplDefaults;
  },
  set(options) {
    validateObject(options, "options");
    return ObjectAssign(inspectReplDefaults, options);
  },
  configurable: true
});
const defaultFG = 39;
const defaultBG = 49;
inspect.colors = {
  __proto__: null,
  reset: [0, 0],
  bold: [1, 22],
  dim: [2, 22],
  italic: [3, 23],
  underline: [4, 24],
  blink: [5, 25],
  inverse: [7, 27],
  hidden: [8, 28],
  strikethrough: [9, 29],
  doubleunderline: [21, 24],
  black: [30, defaultFG],
  red: [31, defaultFG],
  green: [32, defaultFG],
  yellow: [33, defaultFG],
  blue: [34, defaultFG],
  magenta: [35, defaultFG],
  cyan: [36, defaultFG],
  white: [37, defaultFG],
  bgBlack: [40, defaultBG],
  bgRed: [41, defaultBG],
  bgGreen: [42, defaultBG],
  bgYellow: [43, defaultBG],
  bgBlue: [44, defaultBG],
  bgMagenta: [45, defaultBG],
  bgCyan: [46, defaultBG],
  bgWhite: [47, defaultBG],
  framed: [51, 54],
  overlined: [53, 55],
  gray: [90, defaultFG],
  redBright: [91, defaultFG],
  greenBright: [92, defaultFG],
  yellowBright: [93, defaultFG],
  blueBright: [94, defaultFG],
  magentaBright: [95, defaultFG],
  cyanBright: [96, defaultFG],
  whiteBright: [97, defaultFG],
  bgGray: [100, defaultBG],
  bgRedBright: [101, defaultBG],
  bgGreenBright: [102, defaultBG],
  bgYellowBright: [103, defaultBG],
  bgBlueBright: [104, defaultBG],
  bgMagentaBright: [105, defaultBG],
  bgCyanBright: [106, defaultBG],
  bgWhiteBright: [107, defaultBG]
};
function defineColorAlias(target, alias) {
  ObjectDefineProperty(inspect.colors, alias, {
    __proto__: null,
    get() {
      return this[target];
    },
    set(value) {
      this[target] = value;
    },
    configurable: true,
    enumerable: false
  });
}
defineColorAlias("gray", "grey");
defineColorAlias("gray", "blackBright");
defineColorAlias("bgGray", "bgGrey");
defineColorAlias("bgGray", "bgBlackBright");
defineColorAlias("dim", "faint");
defineColorAlias("strikethrough", "crossedout");
defineColorAlias("strikethrough", "strikeThrough");
defineColorAlias("strikethrough", "crossedOut");
defineColorAlias("hidden", "conceal");
defineColorAlias("inverse", "swapColors");
defineColorAlias("inverse", "swapcolors");
defineColorAlias("doubleunderline", "doubleUnderline");
inspect.styles = {
  __proto__: null,
  special: "cyan",
  number: "yellow",
  bigint: "yellow",
  boolean: "yellow",
  undefined: "grey",
  null: "bold",
  string: "green",
  symbol: "green",
  date: "magenta",
  regexp: highlightRegExp,
  module: "underline"
};
inspect.styles.regexp.colors = ["green", "red", "yellow", "cyan", "magenta"];
const highlightRegExpColors = inspect.styles.regexp.colors.slice();
function addQuotes(str, quotes) {
  if (quotes === -1) {
    return `"${str}"`;
  }
  if (quotes === -2) {
    return `\`${str}\``;
  }
  return `'${str}'`;
}
function escapeFn(str) {
  const charCode = StringPrototypeCharCodeAt(str);
  return meta.length > charCode ? meta[charCode] : `\\u${NumberPrototypeToString(charCode, 16)}`;
}
function strEscape(str) {
  let escapeTest = strEscapeSequencesRegExp;
  let escapeReplace = strEscapeSequencesReplacer;
  let singleQuote = 39;
  if (StringPrototypeIncludes(str, "'")) {
    if (!StringPrototypeIncludes(str, '"')) {
      singleQuote = -1;
    } else if (!StringPrototypeIncludes(str, "`") && !StringPrototypeIncludes(str, "${")) {
      singleQuote = -2;
    }
    if (singleQuote !== 39) {
      escapeTest = strEscapeSequencesRegExpSingle;
      escapeReplace = strEscapeSequencesReplacerSingle;
    }
  }
  if (str.length < 5000 && RegExpPrototypeExec(escapeTest, str) === null)
    return addQuotes(str, singleQuote);
  if (str.length > 100) {
    str = RegExpPrototypeSymbolReplace(escapeReplace, str, escapeFn);
    return addQuotes(str, singleQuote);
  }
  let result = "";
  let last = 0;
  for (let i = 0;i < str.length; i++) {
    const point = StringPrototypeCharCodeAt(str, i);
    if (point === singleQuote || point === 92 || point < 32 || point > 126 && point < 160) {
      if (last === i) {
        result += meta[point];
      } else {
        result += `${StringPrototypeSlice(str, last, i)}${meta[point]}`;
      }
      last = i + 1;
    } else if (point >= 55296 && point <= 57343) {
      if (point <= 56319 && i + 1 < str.length) {
        const point = StringPrototypeCharCodeAt(str, i + 1);
        if (point >= 56320 && point <= 57343) {
          i++;
          continue;
        }
      }
      result += `${StringPrototypeSlice(str, last, i)}\\u${NumberPrototypeToString(point, 16)}`;
      last = i + 1;
    }
  }
  if (last !== str.length) {
    result += StringPrototypeSlice(str, last);
  }
  return addQuotes(result, singleQuote);
}
function highlightRegExp(regexpString) {
  const length = regexpString.length;
  let out = "";
  let i = 0;
  let depth = 0;
  let inClass = false;
  const paletteNames = highlightRegExp.colors?.length > 0 ? highlightRegExp.colors : highlightRegExpColors;
  const palette = [];
  for (const name of paletteNames) {
    const color = inspect.colors[name];
    if (color)
      palette.push([`\x1B[${color[0]}m`, `\x1B[${color[1]}m`]);
  }
  function writeGroup(start, end, decreaseDepth = 1) {
    let seq = "";
    i++;
    while (i < length && regexpString[i] !== end) {
      seq += regexpString[i++];
    }
    if (i < length) {
      depth -= decreaseDepth;
      write(start);
      writeDepth(seq, 1, 1);
      write(end);
      depth += decreaseDepth;
    } else {
      writeDepth(start, 1, -seq.length);
    }
  }
  function write(str) {
    const idx = depth % palette.length;
    const color = palette[idx] ?? palette[0];
    out += color[0] + str + color[1];
  }
  function writeDepth(str, incDepth, incI) {
    depth += incDepth;
    write(str);
    depth -= incDepth;
    i += incI;
  }
  write("/");
  depth++;
  i = 1;
  while (i < length) {
    const ch = regexpString[i];
    if (inClass) {
      if (ch === "\\") {
        let seq = "\\";
        i++;
        if (i < length) {
          seq += regexpString[i++];
          const next = seq[1];
          if (next === "u" && regexpString[i] === "{") {
            writeGroup(`${seq}{`, "}", 0);
            continue;
          } else if ((next === "p" || next === "P") && regexpString[i] === "{") {
            writeGroup(`${seq}{`, "}", 0);
            continue;
          } else if (seq[1] === "x") {
            seq += regexpString.slice(i, i + 2);
            i += 2;
          }
        }
        write(seq);
      } else if (ch === "]") {
        depth--;
        write("]");
        i++;
        inClass = false;
      } else if (ch === "-" && regexpString[i - 1] !== "[" && i + 1 < length && regexpString[i + 1] !== "]") {
        writeDepth("-", 1, 1);
      } else {
        write(ch);
        i++;
      }
    } else if (ch === "[") {
      write("[");
      depth++;
      i++;
      inClass = true;
    } else if (ch === "(") {
      write("(");
      depth++;
      i++;
      if (i < length && regexpString[i] === "?") {
        i++;
        const a = i < length ? regexpString[i] : "";
        if (a === ":" || a === "=" || a === "!") {
          writeDepth(`?${a}`, -1, 1);
        } else {
          const b = i + 1 < length ? regexpString[i + 1] : "";
          if (a === "<" && (b === "=" || b === "!")) {
            writeDepth(`?<${b}`, -1, 2);
          } else if (a === "<") {
            i++;
            const start = i;
            while (i < length && regexpString[i] !== ">") {
              i++;
            }
            const name = regexpString.slice(start, i);
            if (i < length && regexpString[i] === ">") {
              depth--;
              write("?<");
              writeDepth(name, 1, 0);
              write(">");
              depth++;
              i++;
            } else {
              writeDepth("?<", -1, 0);
              write(name);
            }
          } else {
            write("?");
          }
        }
      }
    } else if (ch === ")") {
      depth--;
      write(")");
      i++;
    } else if (ch === "\\") {
      let seq = "\\";
      i++;
      if (i < length) {
        seq += regexpString[i++];
        const next = seq[1];
        if (i < length) {
          if (next === "u" && regexpString[i] === "{") {
            writeGroup(`${seq}{`, "}", 0);
            continue;
          } else if (next === "x") {
            seq += regexpString.slice(i, i + 2);
            i += 2;
          } else if (next >= "0" && next <= "9") {
            while (i < length && regexpString[i] >= "0" && regexpString[i] <= "9") {
              seq += regexpString[i++];
            }
          } else if (next === "k" && regexpString[i] === "<") {
            writeGroup(`${seq}<`, ">");
            continue;
          } else if ((next === "p" || next === "P") && regexpString[i] === "{") {
            writeGroup(`${seq}{`, "}", 0);
            continue;
          }
        }
      }
      writeDepth(seq, 1, 0);
    } else if (ch === "|" || ch === "+" || ch === "*" || ch === "?" || ch === "," || ch === "^" || ch === "$") {
      writeDepth(ch, 3, 1);
    } else if (ch === "{") {
      i++;
      let digits = "";
      while (i < length && regexpString[i] >= "0" && regexpString[i] <= "9") {
        digits += regexpString[i++];
      }
      if (digits) {
        write("{");
        depth++;
        writeDepth(digits, 1, 0);
      }
      if (i < length) {
        if (regexpString[i] === ",") {
          if (!digits) {
            write("{");
            depth++;
          }
          write(",");
          i++;
        } else if (!digits) {
          depth += 1;
          write("{");
          depth -= 1;
          continue;
        }
      }
      let digits2 = "";
      while (i < length && regexpString[i] >= "0" && regexpString[i] <= "9") {
        digits2 += regexpString[i++];
      }
      if (digits2) {
        writeDepth(digits2, 1, 0);
      }
      if (i < length && regexpString[i] === "}") {
        depth--;
        write("}");
        i++;
      }
      if (i < length && regexpString[i] === "?") {
        writeDepth("?", 3, 1);
      }
    } else if (ch === ".") {
      writeDepth(ch, 2, 1);
    } else if (ch === "/") {
      break;
    } else {
      writeDepth(ch, 1, 1);
    }
  }
  writeDepth("/", -1, 1);
  if (i < length) {
    write(regexpString.slice(i));
  }
  return out;
}
function stylizeWithColor(str, styleType) {
  const style = inspect.styles[styleType];
  if (style !== undefined) {
    if (typeof style === "function")
      return style(str);
    const color = inspect.colors[style];
    if (color !== undefined)
      return `\x1B[${color[0]}m${str}\x1B[${color[1]}m`;
  }
  return str;
}
function stylizeNoColor(str) {
  return str;
}
function getEmptyFormatArray() {
  return [];
}
function isInstanceof(object, proto) {
  try {
    return object instanceof proto;
  } catch {
    return false;
  }
}
let wellKnownPrototypes;
function initializeWellKnownPrototypes() {
  wellKnownPrototypes = new SafeMap;
  wellKnownPrototypes.set(Array.prototype, { name: "Array", constructor: Array });
  wellKnownPrototypes.set(ArrayBuffer.prototype, { name: "ArrayBuffer", constructor: ArrayBuffer });
  wellKnownPrototypes.set(Function.prototype, { name: "Function", constructor: Function });
  wellKnownPrototypes.set(Map.prototype, { name: "Map", constructor: Map });
  wellKnownPrototypes.set(Object.prototype, { name: "Object", constructor: Object });
  wellKnownPrototypes.set(Set.prototype, { name: "Set", constructor: Set });
}
function getConstructorName(obj, ctx, recurseTimes, protoProps) {
  let firstProto;
  const tmp = obj;
  if (!wellKnownPrototypes)
    initializeWellKnownPrototypes();
  while (obj || isUndetectableObject(obj)) {
    const wellKnownPrototypeNameAndConstructor = wellKnownPrototypes.get(obj);
    if (wellKnownPrototypeNameAndConstructor != null) {
      const { name, constructor } = wellKnownPrototypeNameAndConstructor;
      if (tmp instanceof constructor) {
        if (protoProps !== undefined && firstProto !== obj) {
          addPrototypeProperties(ctx, tmp, firstProto || tmp, recurseTimes, protoProps);
        }
        return name;
      }
    }
    const descriptor = ObjectGetOwnPropertyDescriptor(obj, "constructor");
    const descriptorValue = descriptor !== undefined ? descriptor.value : undefined;
    if (typeof descriptorValue === "function") {
      const descriptorValueName = descriptorValue.name;
      if (descriptorValueName !== "" && isInstanceof(tmp, descriptorValue)) {
        if (protoProps !== undefined && (firstProto !== obj || !builtInObjects.has(descriptorValueName))) {
          addPrototypeProperties(ctx, tmp, firstProto || tmp, recurseTimes, protoProps);
        }
        return String(descriptorValueName);
      }
    }
    obj = ObjectGetPrototypeOf(obj);
    if (firstProto === undefined) {
      firstProto = obj;
    }
  }
  if (firstProto === null) {
    return null;
  }
  const res = internalGetConstructorName(tmp);
  if (recurseTimes > ctx.depth && ctx.depth !== null) {
    return `${res} <Complex prototype>`;
  }
  const protoConstr = getConstructorName(firstProto, ctx, recurseTimes + 1, protoProps);
  if (protoConstr === null) {
    return `${res} <${inspect(firstProto, {
      ...ctx,
      customInspect: false,
      depth: -1
    })}>`;
  }
  return `${res} <${protoConstr}>`;
}
function addPrototypeProperties(ctx, main, obj, recurseTimes, output) {
  let depth = 0;
  let keys;
  let keySet;
  do {
    if (depth !== 0 || main === obj) {
      obj = ObjectGetPrototypeOf(obj);
      if (obj === null) {
        return;
      }
      const descriptor = ObjectGetOwnPropertyDescriptor(obj, "constructor");
      if (descriptor !== undefined && typeof descriptor.value === "function" && builtInObjects.has(descriptor.value.name)) {
        return;
      }
    }
    if (depth === 0) {
      keySet = new SafeSet;
    } else {
      ArrayPrototypeForEach(keys, (key) => keySet.add(key));
    }
    keys = ReflectOwnKeys(obj);
    ArrayPrototypePush.call(ctx.seen, main);
    for (const key of keys) {
      if (key === "constructor" || ObjectPrototypeHasOwnProperty(main, key) || depth !== 0 && keySet.has(key)) {
        continue;
      }
      const desc = ObjectGetOwnPropertyDescriptor(obj, key);
      if (typeof desc.value === "function") {
        continue;
      }
      const value = formatProperty(ctx, obj, recurseTimes, key, kObjectType, desc, main);
      if (ctx.colors) {
        ArrayPrototypePush.call(output, `\x1B[2m${value}\x1B[22m`);
      } else {
        ArrayPrototypePush.call(output, value);
      }
    }
    ArrayPrototypePop(ctx.seen);
  } while (++depth !== 3);
}
function getPrefix(constructor, tag, fallback, size = "") {
  if (constructor === null) {
    if (tag !== "" && fallback !== tag) {
      return `[${fallback}${size}: null prototype] [${tag}] `;
    }
    return `[${fallback}${size}: null prototype] `;
  }
  if (tag !== "" && constructor !== tag) {
    return `${constructor}${size} [${tag}] `;
  }
  return `${constructor}${size} `;
}
function getKeys(value, showHidden) {
  let keys;
  const symbols = ObjectGetOwnPropertySymbols(value);
  if (showHidden) {
    keys = ObjectGetOwnPropertyNames(value);
    if (symbols.length !== 0)
      ArrayPrototypePush.apply(keys, symbols);
  } else {
    try {
      keys = ObjectKeys(value);
    } catch (err) {
      assert(isNativeError(err) && err.name === "ReferenceError" && isModuleNamespaceObject(value));
      keys = ObjectGetOwnPropertyNames(value);
    }
    if (symbols.length !== 0) {
      const filter = (key) => ObjectPrototypePropertyIsEnumerable(value, key);
      ArrayPrototypePush.apply(keys, ArrayPrototypeFilter(symbols, filter));
    }
  }
  return keys;
}
function getCtxStyle(value, constructor, tag) {
  let fallback = "";
  if (constructor === null) {
    fallback = internalGetConstructorName(value);
    if (fallback === tag) {
      fallback = "Object";
    }
  }
  return getPrefix(constructor, tag, fallback);
}
function formatProxy(ctx, proxy, recurseTimes) {
  if (recurseTimes > ctx.depth && ctx.depth !== null) {
    return ctx.stylize("Proxy [Array]", "special");
  }
  recurseTimes += 1;
  ctx.indentationLvl += 2;
  const res = [formatValue(ctx, proxy[0], recurseTimes), formatValue(ctx, proxy[1], recurseTimes)];
  ctx.indentationLvl -= 2;
  return reduceToSingleString(ctx, res, "", ["Proxy [", "]"], kArrayExtrasType, recurseTimes);
}
function formatValue(ctx, value, recurseTimes, typedArray) {
  if (typeof value !== "object" && typeof value !== "function" && !isUndetectableObject(value)) {
    return formatPrimitive(ctx.stylize, value, ctx);
  }
  if (value === null) {
    return ctx.stylize("null", "null");
  }
  const context = value;
  const proxy = getProxyDetails(value, !!ctx.showProxy);
  if (proxy !== undefined) {
    if (proxy === null || proxy[0] === null) {
      return ctx.stylize("<Revoked Proxy>", "special");
    }
    if (ctx.showProxy) {
      return formatProxy(ctx, proxy, recurseTimes);
    }
    value = proxy;
  }
  if (ctx.customInspect) {
    const maybeCustom = value[customInspectSymbol];
    if (typeof maybeCustom === "function" && maybeCustom !== inspect && Object.getOwnPropertyDescriptor(value, "constructor")?.value?.prototype !== value) {
      const depth = ctx.depth === null ? null : ctx.depth - recurseTimes;
      const isCrossContext = proxy !== undefined || !(context instanceof Object);
      const ret = maybeCustom.call(context, depth, getUserOptions(ctx, isCrossContext), inspect);
      if (ret !== context) {
        if (typeof ret !== "string") {
          return formatValue(ctx, ret, recurseTimes);
        }
        return StringPrototypeReplaceAll(ret, `
`, `
${StringPrototypeRepeat(" ", ctx.indentationLvl)}`);
      }
    }
  }
  if (ctx.seen.includes(value)) {
    let index = 1;
    if (ctx.circular === undefined) {
      ctx.circular = new SafeMap;
      ctx.circular.set(value, index);
    } else {
      index = ctx.circular.get(value);
      if (index === undefined) {
        index = ctx.circular.size + 1;
        ctx.circular.set(value, index);
      }
    }
    return ctx.stylize(`[Circular *${index}]`, "special");
  }
  return formatRaw(ctx, value, recurseTimes, typedArray);
}
function formatRaw(ctx, value, recurseTimes, typedArray) {
  let keys;
  let protoProps;
  if (ctx.showHidden && (recurseTimes <= ctx.depth || ctx.depth === null)) {
    protoProps = [];
  }
  const constructor = getConstructorName(value, ctx, recurseTimes, protoProps);
  if (protoProps !== undefined && protoProps.length === 0) {
    protoProps = undefined;
  }
  let tag = value[SymbolToStringTag];
  if (typeof tag !== "string" || tag !== "" && (ctx.showHidden ? ObjectPrototypeHasOwnProperty : ObjectPrototypePropertyIsEnumerable)(value, SymbolToStringTag)) {
    tag = "";
  }
  let base = "";
  let formatter = getEmptyFormatArray;
  let braces;
  let extraKeys;
  let noIterator = true;
  let i = 0;
  const filter = ctx.showHidden ? 0 : 2;
  let extrasType = kObjectType;
  if (SymbolIterator in value || constructor === null) {
    noIterator = false;
    if (host.isJSArray(value)) {
      const prefix = constructor !== "Array" || tag !== "" ? getPrefix(constructor, tag, "Array", `(${value.length})`) : "";
      keys = getOwnNonIndexProperties(value, filter);
      braces = [`${prefix}[`, "]"];
      if (value.length === 0 && keys.length === 0 && protoProps === undefined)
        return `${braces[0]}]`;
      extrasType = kArrayExtrasType;
      formatter = formatArray;
    } else if (isSet(value)) {
      const size = SetPrototypeGetSize(value);
      const prefix = getPrefix(constructor, tag, "Set", `(${size})`);
      keys = getKeys(value, ctx.showHidden);
      formatter = constructor !== null ? FunctionPrototypeBind(formatSet, null, value) : FunctionPrototypeBind(formatSet, null, SetPrototypeValues(value));
      if (size === 0 && keys.length === 0 && protoProps === undefined)
        return `${prefix}{}`;
      braces = [`${prefix}{`, "}"];
    } else if (isMap(value)) {
      const size = MapPrototypeGetSize(value);
      const prefix = getPrefix(constructor, tag, "Map", `(${size})`);
      keys = getKeys(value, ctx.showHidden);
      formatter = constructor !== null ? FunctionPrototypeBind(formatMap, null, value) : FunctionPrototypeBind(formatMap, null, MapPrototypeEntries(value));
      if (size === 0 && keys.length === 0 && protoProps === undefined)
        return `${prefix}{}`;
      braces = [`${prefix}{`, "}"];
    } else if (isTypedArray(value)) {
      keys = getOwnNonIndexProperties(value, filter);
      let bound = value;
      let fallback = "";
      if (constructor === null) {
        fallback = TypedArrayPrototypeGetSymbolToStringTag(value);
        bound = new primordials[fallback](value);
      }
      const size = TypedArrayPrototypeGetLength(value);
      const prefix = getPrefix(constructor, tag, fallback, `(${size})`);
      braces = [`${prefix}[`, "]"];
      if (value.length === 0 && keys.length === 0 && !ctx.showHidden)
        return `${braces[0]}]`;
      formatter = FunctionPrototypeBind(formatTypedArray, null, bound, size);
      extrasType = kArrayExtrasType;
      if (ctx.showHidden) {
        extraKeys = ["BYTES_PER_ELEMENT", "length", "byteLength", "byteOffset", "buffer"];
        typedArray = true;
      }
    } else if (isMapIterator(value)) {
      keys = getKeys(value, ctx.showHidden);
      braces = getIteratorBraces("Map", tag);
      formatter = FunctionPrototypeBind(formatIterator, null, braces);
    } else if (isSetIterator(value)) {
      keys = getKeys(value, ctx.showHidden);
      braces = getIteratorBraces("Set", tag);
      formatter = FunctionPrototypeBind(formatIterator, null, braces);
    } else {
      noIterator = true;
    }
  }
  if (noIterator) {
    keys = getKeys(value, ctx.showHidden);
    braces = ["{", "}"];
    if (typeof value === "function") {
      base = getFunctionBase(ctx, value, constructor, tag);
      if (keys.length === 0 && protoProps === undefined)
        return ctx.stylize(base, "special");
    } else if (constructor === "Object") {
      if (isArgumentsObject(value)) {
        braces[0] = "[Arguments] {";
      } else if (tag !== "") {
        braces[0] = `${getPrefix(constructor, tag, "Object")}{`;
      }
      if (keys.length === 0 && protoProps === undefined) {
        return `${braces[0]}}`;
      }
    } else if (isRegExp(value)) {
      base = RegExpPrototypeToString(constructor !== null ? value : new RegExp(value));
      const prefix = getPrefix(constructor, tag, "RegExp");
      if (prefix !== "RegExp ")
        base = `${prefix}${base}`;
      if (keys.length === 0 && protoProps === undefined || recurseTimes > ctx.depth && ctx.depth !== null) {
        return ctx.stylize(base, "regexp");
      }
    } else if (isDate(value)) {
      base = NumberIsNaN(DatePrototypeGetTime(value)) ? DatePrototypeToString(value) : DatePrototypeToISOString(value);
      const prefix = getPrefix(constructor, tag, "Date");
      if (prefix !== "Date ")
        base = `${prefix}${base}`;
      if (keys.length === 0 && protoProps === undefined) {
        return ctx.stylize(base, "date");
      }
    } else if (isNativeError(value) || value instanceof Error) {
      base = formatError(value, constructor, tag, ctx, keys);
      if (keys.length === 0 && protoProps === undefined)
        return base;
    } else if (isAnyArrayBuffer(value)) {
      const arrayType = isArrayBuffer(value) ? "ArrayBuffer" : "SharedArrayBuffer";
      const prefix = getPrefix(constructor, tag, arrayType);
      if (typedArray === undefined) {
        formatter = formatArrayBuffer;
      } else if (keys.length === 0 && protoProps === undefined) {
        return prefix + `{ [byteLength]: ${formatNumber(ctx.stylize, value.byteLength, false)} }`;
      }
      braces[0] = `${prefix}{`;
      extraKeys = ["byteLength"];
    } else if (isDataView(value)) {
      braces[0] = `${getPrefix(constructor, tag, "DataView")}{`;
      extraKeys = ["byteLength", "byteOffset", "buffer"];
    } else if (isPromise(value)) {
      braces[0] = `${getPrefix(constructor, tag, "Promise")}{`;
      formatter = formatPromise;
    } else if (isWeakSet(value)) {
      braces[0] = `${getPrefix(constructor, tag, "WeakSet")}{`;
      formatter = ctx.showHidden ? formatWeakSet : formatWeakCollection;
    } else if (isWeakMap(value)) {
      braces[0] = `${getPrefix(constructor, tag, "WeakMap")}{`;
      formatter = ctx.showHidden ? formatWeakMap : formatWeakCollection;
    } else if (isModuleNamespaceObject(value)) {
      braces[0] = `${getPrefix(constructor, tag, "Module")}{`;
      formatter = formatNamespaceObject.bind(null, keys);
    } else if (isBoxedPrimitive(value)) {
      base = getBoxedBase(value, ctx, keys, constructor, tag);
      if (keys.length === 0 && protoProps === undefined) {
        return base;
      }
    } else {
      if (keys.length === 0 && protoProps === undefined) {
        if (isExternal(value)) {
          const address = "0";
          return ctx.stylize("[External: 0]", "special");
        }
        return `${getCtxStyle(value, constructor, tag)}{}`;
      }
      braces[0] = `${getCtxStyle(value, constructor, tag)}{`;
    }
  }
  if (recurseTimes > ctx.depth && ctx.depth !== null) {
    let constructorName = StringPrototypeSlice(getCtxStyle(value, constructor, tag), 0, -1);
    if (constructor !== null)
      constructorName = `[${constructorName}]`;
    return ctx.stylize(constructorName, "special");
  }
  recurseTimes += 1;
  ctx.seen.push(value);
  ctx.currentDepth = recurseTimes;
  let output;
  const indentationLvl = ctx.indentationLvl;
  try {
    if (ctx.currentDepth > 1000)
      throw new RangeError(ERROR_STACK_OVERFLOW_MSG);
    output = formatter(ctx, value, recurseTimes);
    if (extraKeys !== undefined) {
      for (i = 0;i < extraKeys.length; i++) {
        let formatted;
        try {
          formatted = formatExtraProperties(ctx, value, recurseTimes, extraKeys[i], typedArray);
        } catch {
          const tempValue = { [extraKeys[i]]: value.buffer[extraKeys[i]] };
          formatted = formatExtraProperties(ctx, tempValue, recurseTimes, extraKeys[i], typedArray);
        }
        ArrayPrototypePush.call(output, formatted);
      }
    }
    for (i = 0;i < keys.length; i++) {
      ArrayPrototypePush.call(output, formatProperty(ctx, value, recurseTimes, keys[i], extrasType));
    }
    if (protoProps !== undefined) {
      ArrayPrototypePush.apply(output, protoProps);
    }
  } catch (err) {
    if (err instanceof RangeError && err.message === ERROR_STACK_OVERFLOW_MSG) {
      const constructorName = StringPrototypeSlice(getCtxStyle(value, constructor, tag), 0, -1);
      ctx.seen.pop();
      ctx.indentationLvl = indentationLvl;
      return ctx.stylize(`[${constructorName}: Inspection interrupted prematurely. Maximum call stack size exceeded.]`, "special");
    }
    throw new AssertionError("handleMaxCallStackSize assertion failed: " + String(err), true);
  }
  const circular = ctx.circular;
  if (circular !== undefined) {
    const index = circular.get(value);
    if (index !== undefined) {
      ctx.seenRefs ??= new Set;
      const SEEN = ctx.seenRefs.has(index);
      if (!SEEN) {
        ctx.seenRefs.add(index);
        const reference = ctx.stylize(`<ref *${index}>`, "special");
        if (ctx.compact !== true) {
          base = base === "" ? reference : `${reference} ${base}`;
        } else {
          braces[0] = `${reference} ${braces[0]}`;
        }
      } else {
        //! this is a non-standard behavior compared to Node's implementation
        const reference = ctx.stylize(`[Circular *${index}]`, "special");
        return reference;
      }
    }
  }
  ctx.seen.pop();
  const ctxSorted = ctx.sorted;
  if (ctxSorted) {
    const comparator = ctxSorted === true ? undefined : ctxSorted;
    if (extrasType === kObjectType) {
      ArrayPrototypeSort(output, comparator);
    } else {
      const keysLength = keys.length;
      if (keysLength > 1) {
        const sorted = ArrayPrototypeSort(ArrayPrototypeSlice(output, output.length - keysLength), comparator);
        ArrayPrototypeUnshift(sorted, output, output.length - keysLength, keysLength);
        ArrayPrototypeSplice.apply(null, sorted);
      }
    }
  }
  const res = reduceToSingleString(ctx, output, base, braces, extrasType, recurseTimes, value);
  const budget = ctx.budget[ctx.indentationLvl] || 0;
  const newLength = budget + res.length;
  ctx.budget[ctx.indentationLvl] = newLength;
  if (newLength > 134217728) {
    ctx.depth = -1;
  }
  return res;
}
function getIteratorBraces(type, tag) {
  if (tag !== `${type} Iterator`) {
    if (tag !== "")
      tag += "] [";
    tag += `${type} Iterator`;
  }
  return [`[${tag}] {`, "}"];
}
function getBoxedBase(value, ctx, keys, constructor, tag) {
  let fn;
  let type;
  if (isNumberObject(value)) {
    fn = NumberPrototypeValueOf;
    type = "Number";
  } else if (isStringObject(value)) {
    fn = StringPrototypeValueOf;
    type = "String";
    keys.splice(0, value.length);
  } else if (isBooleanObject(value)) {
    fn = BooleanPrototypeValueOf;
    type = "Boolean";
  } else if (isBigIntObject(value)) {
    fn = BigIntPrototypeValueOf;
    type = "BigInt";
  } else {
    fn = SymbolPrototypeValueOf;
    type = "Symbol";
  }
  let base = `[${type}`;
  if (type !== constructor) {
    if (constructor === null) {
      base += " (null prototype)";
    } else {
      base += ` (${constructor})`;
    }
  }
  base += `: ${formatPrimitive(stylizeNoColor, fn(value), ctx)}]`;
  if (tag !== "" && tag !== constructor) {
    base += ` [${tag}]`;
  }
  if (keys.length !== 0 || ctx.stylize === stylizeNoColor)
    return base;
  return ctx.stylize(base, StringPrototypeToLowerCase(type));
}
function getClassBase(value, constructor, tag) {
  const hasName = ObjectPrototypeHasOwnProperty(value, "name");
  const name = hasName && value.name || "(anonymous)";
  let base = `class ${name}`;
  if (constructor !== "Function" && constructor !== null) {
    base += ` [${constructor}]`;
  }
  if (tag !== "" && constructor !== tag) {
    base += ` [${tag}]`;
  }
  if (constructor !== null) {
    const superName = ObjectGetPrototypeOf(value).name;
    if (superName) {
      base += ` extends ${superName}`;
    }
  } else {
    base += " extends [null prototype]";
  }
  return `[${base}]`;
}
function getFunctionBase(ctx, value, constructor, tag) {
  const stringified = FunctionPrototypeToString(value);
  if (StringPrototypeStartsWith(stringified, "class") && stringified[stringified.length - 1] === "}") {
    const slice = StringPrototypeSlice(stringified, 5, -1);
    const bracketIndex = StringPrototypeIndexOf(slice, "{");
    if (bracketIndex !== -1 && (!StringPrototypeIncludes(StringPrototypeSlice(slice, 0, bracketIndex), "(") || RegExpPrototypeExec(classRegExp, RegExpPrototypeSymbolReplace(stripCommentsRegExp, slice)) !== null)) {
      return getClassBase(value, constructor, tag);
    }
  }
  let type = "Function";
  if (isGeneratorFunction(value)) {
    type = `Generator${type}`;
  }
  if (isAsyncFunction(value)) {
    type = `Async${type}`;
  }
  let base = `[${type}`;
  if (constructor === null) {
    base += " (null prototype)";
  }
  if (value.name === "") {
    base += " (anonymous)";
  } else {
    base += `: ${typeof value.name === "string" ? value.name : formatValue(ctx, value.name)}`;
  }
  base += "]";
  if (constructor !== type && constructor !== null) {
    base += ` ${constructor}`;
  }
  if (tag !== "" && constructor !== tag) {
    base += ` [${tag}]`;
  }
  return base;
}
function identicalSequenceRange(a, b) {
  for (let i = 0;i < a.length - 3; i++) {
    const pos = ArrayPrototypeIndexOf.call(b, a[i]);
    if (pos !== -1) {
      const rest = b.length - pos;
      if (rest > 3) {
        let len = 1;
        const maxLen = MathMinIntrinsic(a.length - i, rest);
        while (maxLen > len && a[i + len] === b[pos + len]) {
          len++;
        }
        if (len > 3) {
          return { len, offset: i };
        }
      }
    }
  }
  return { len: 0, offset: 0 };
}
function getStackString(error) {
  return error.stack ? String(error.stack) : ErrorPrototypeToString(error);
}
function getStackFrames(ctx, err, stack) {
  const frames = StringPrototypeSplit(stack, `
`);
  let cause;
  try {
    ({ cause } = err);
  } catch {}
  if (cause != null && (isNativeError(cause) || cause instanceof Error)) {
    const causeStack = getStackString(cause);
    const causeStackStart = StringPrototypeIndexOf(causeStack, `
    at`);
    if (causeStackStart !== -1) {
      const causeFrames = StringPrototypeSplit(StringPrototypeSlice(causeStack, causeStackStart + 1), `
`);
      const { len, offset } = identicalSequenceRange(frames, causeFrames);
      if (len > 0) {
        const skipped = len - 2;
        const msg = `    ... ${skipped} lines matching cause stack trace ...`;
        frames.splice(offset + 1, skipped, ctx.stylize(msg, "undefined"));
      }
    }
  }
  return frames;
}
function improveStack(stack, constructor, name, tag) {
  let len = name.length;
  if (constructor === null || StringPrototypeEndsWith(name, "Error") && StringPrototypeStartsWith(stack, name) && (stack.length === len || stack[len] === ":" || stack[len] === `
`)) {
    let fallback = "Error";
    if (constructor === null) {
      const start = RegExpPrototypeExec(/^([A-Z][a-z_ A-Z0-9[\]()-]+)(?::|\n {4}at)/, stack) || RegExpPrototypeExec(/^([a-z_A-Z0-9-]*Error)$/, stack);
      fallback = start?.[1] || "";
      len = fallback.length;
      fallback ||= "Error";
    }
    const prefix = StringPrototypeSlice(getPrefix(constructor, tag, fallback), 0, -1);
    if (name !== prefix) {
      if (StringPrototypeIncludes(prefix, name)) {
        if (len === 0) {
          stack = `${prefix}: ${stack}`;
        } else {
          stack = `${prefix}${StringPrototypeSlice(stack, len)}`;
        }
      } else {
        stack = `${prefix} [${name}]${StringPrototypeSlice(stack, len)}`;
      }
    }
  }
  return stack;
}
function removeDuplicateErrorKeys(ctx, keys, err, stack) {
  if (!ctx.showHidden && keys.length !== 0) {
    for (const name of ["name", "message", "stack"]) {
      const index = ArrayPrototypeIndexOf(keys, name);
      if (index !== -1 && StringPrototypeIncludes(stack, err[name])) {
        ArrayPrototypeSplice(keys, index, 1);
      }
    }
  }
}
function markNodeModules(ctx, line) {
  let tempLine = "";
  let nodeModule;
  let pos = 0;
  while ((nodeModule = nodeModulesRegExp.exec(line)) !== null) {
    tempLine += StringPrototypeSlice(line, pos, nodeModule.index + 14);
    tempLine += ctx.stylize(nodeModule[1], "module");
    pos = nodeModule.index + nodeModule[0].length;
  }
  if (pos !== 0) {
    line = tempLine + StringPrototypeSlice(line, pos);
  }
  return line;
}
function markCwd(ctx, line, workingDirectory) {
  let cwdStartPos = StringPrototypeIndexOf(line, workingDirectory);
  let tempLine = "";
  let cwdLength = workingDirectory.length;
  if (cwdStartPos !== -1) {
    if (StringPrototypeSlice(line, cwdStartPos - 7, cwdStartPos) === "file://") {
      cwdLength += 7;
      cwdStartPos -= 7;
    }
    const start = line[cwdStartPos - 1] === "(" ? cwdStartPos - 1 : cwdStartPos;
    const end = start !== cwdStartPos && StringPrototypeEndsWith(line, ")") ? -1 : line.length;
    const workingDirectoryEndPos = cwdStartPos + cwdLength + 1;
    const cwdSlice = StringPrototypeSlice(line, start, workingDirectoryEndPos);
    tempLine += StringPrototypeSlice(line, 0, start);
    tempLine += ctx.stylize(cwdSlice, "undefined");
    tempLine += StringPrototypeSlice(line, workingDirectoryEndPos, end);
    if (end === -1) {
      tempLine += ctx.stylize(")", "undefined");
    }
  } else {
    tempLine += line;
  }
  return tempLine;
}
function safeGetCWD() {
  let workingDirectory;
  try {
    workingDirectory = process.cwd();
  } catch {
    return;
  }
  return workingDirectory;
}
function formatError(err, constructor, tag, ctx, keys) {
  const name = err.name != null ? String(err.name) : "Error";
  let stack = getStackString(err);
  //! temp fix for Bun losing the error name from inherited errors + extraneous ": " with no message
  stack = stack.replace(/^Error: /, `${name}${err.message ? ": " : ""}`);
  removeDuplicateErrorKeys(ctx, keys, err, stack);
  if ("cause" in err && (keys.length === 0 || !ArrayPrototypeIncludes(keys, "cause"))) {
    ArrayPrototypePush.call(keys, "cause");
  }
  if (host.isJSArray(err.errors) && (keys.length === 0 || !ArrayPrototypeIncludes(keys, "errors"))) {
    ArrayPrototypePush.call(keys, "errors");
  }
  stack = improveStack(stack, constructor, name, tag);
  let pos = err.message && StringPrototypeIndexOf(stack, err.message) || -1;
  if (pos !== -1)
    pos += err.message.length;
  const stackStart = StringPrototypeIndexOf(stack, `
    at`, pos);
  if (stackStart === -1) {
    stack = `[${stack}]`;
  } else {
    let newStack = StringPrototypeSlice(stack, 0, stackStart);
    const stackFramePart = StringPrototypeSlice(stack, stackStart + 1);
    const lines = getStackFrames(ctx, err, stackFramePart);
    if (ctx.colors) {
      const workingDirectory = safeGetCWD();
      let esmWorkingDirectory;
      for (let line of lines) {
        const core = RegExpPrototypeExec(coreModuleRegExp, line);
        if (core !== null && (StringPrototypeStartsWith(core[1], "internal/") || ArrayPrototypeIncludes(host.requireBuiltin("node:module").builtinModules, core[1]))) {
          newStack += `
${ctx.stylize(line, "undefined")}`;
        } else {
          newStack += `
`;
          line = markNodeModules(ctx, line);
          if (workingDirectory !== undefined) {
            let newLine = markCwd(ctx, line, workingDirectory);
            if (newLine === line) {
              esmWorkingDirectory ??= host.requireBuiltin("node:url").pathToFileURL(workingDirectory).href;
              newLine = markCwd(ctx, line, esmWorkingDirectory);
            }
            line = newLine;
          }
          newStack += line;
        }
      }
    } else {
      newStack += `
${ArrayPrototypeJoin(lines, `
`)}`;
    }
    stack = newStack;
  }
  const indentationLvl = ctx.indentationLvl;
  if (indentationLvl !== 0) {
    const indentation = StringPrototypeRepeat(" ", indentationLvl);
    stack = StringPrototypeReplaceAll(stack, `
`, `
${indentation}`);
  }
  return stack;
}
function groupArrayElements(ctx, output, value) {
  let totalLength = 0;
  let maxLength = 0;
  let i = 0;
  let outputLength = output.length;
  if (ctx.maxArrayLength < output.length) {
    outputLength--;
  }
  const separatorSpace = 2;
  const dataLen = new Array(outputLength);
  for (;i < outputLength; i++) {
    const len = getStringWidth(output[i], ctx.colors);
    dataLen[i] = len;
    totalLength += len + separatorSpace;
    if (maxLength < len)
      maxLength = len;
  }
  const actualMax = maxLength + separatorSpace;
  const indentationLvl = ctx.indentationLvl;
  const breakLength = ctx.breakLength;
  if (actualMax * 3 + indentationLvl < breakLength && (totalLength / actualMax > 5 || maxLength <= 6)) {
    const approxCharHeights = 2.5;
    const averageBias = MathSqrt(actualMax - totalLength / output.length);
    const biasedMax = MathMax(actualMax - 3 - averageBias, 1);
    const columns = MathMinIntrinsic(MathRound(MathSqrt(2.5 * biasedMax * outputLength) / biasedMax), MathFloor((breakLength - indentationLvl) / actualMax), ctx.compact * 4, 15);
    if (columns <= 1) {
      return output;
    }
    const tmp = [];
    const maxLineLength = [];
    for (let i = 0;i < columns; i++) {
      let lineMaxLength = 0;
      for (let j = i;j < output.length; j += columns) {
        if (dataLen[j] > lineMaxLength)
          lineMaxLength = dataLen[j];
      }
      lineMaxLength += separatorSpace;
      maxLineLength[i] = lineMaxLength;
    }
    let order = StringPrototypePadStart;
    if (value !== undefined) {
      for (let i = 0;i < output.length; i++) {
        if (typeof value[i] !== "number" && typeof value[i] !== "bigint") {
          order = StringPrototypePadEnd;
          break;
        }
      }
    }
    for (let i = 0;i < outputLength; i += columns) {
      const max = MathMinIntrinsic(i + columns, outputLength);
      let str = "";
      let j = i;
      for (;j < max - 1; j++) {
        const padding = maxLineLength[j - i] + output[j].length - dataLen[j];
        str += order(`${output[j]}, `, padding, " ");
      }
      if (order === StringPrototypePadStart) {
        const padding = maxLineLength[j - i] + output[j].length - dataLen[j] - separatorSpace;
        str += StringPrototypePadStart(output[j], padding, " ");
      } else {
        str += output[j];
      }
      ArrayPrototypePush.call(tmp, str);
    }
    if (ctx.maxArrayLength < output.length) {
      ArrayPrototypePush.call(tmp, output[outputLength]);
    }
    output = tmp;
  }
  return output;
}
function addNumericSeparator(integerString) {
  let result = "";
  let i = integerString.length;
  const start = integerString[0] === "-" ? 1 : 0;
  for (;i >= start + 4; i -= 3) {
    result = `_${StringPrototypeSlice(integerString, i - 3, i)}${result}`;
  }
  return i === integerString.length ? integerString : `${StringPrototypeSlice(integerString, 0, i)}${result}`;
}
function addNumericSeparatorEnd(integerString) {
  let result = "";
  let i = 0;
  for (;i < integerString.length - 3; i += 3) {
    result += `${StringPrototypeSlice(integerString, i, i + 3)}_`;
  }
  return i === 0 ? integerString : `${result}${StringPrototypeSlice(integerString, i)}`;
}
const remainingText = (remaining) => `... ${remaining} more item${remaining > 1 ? "s" : ""}`;
function formatNumber(fn, number, numericSeparator) {
  if (!numericSeparator) {
    if (ObjectIs(number, -0)) {
      return fn("-0", "number");
    }
    return fn(`${number}`, "number");
  }
  const integer = MathTrunc(number);
  const string = String(integer);
  if (integer === number) {
    if (!NumberIsFinite(number) || StringPrototypeIncludes(string, "e")) {
      return fn(string, "number");
    }
    return fn(`${addNumericSeparator(string)}`, "number");
  }
  if (NumberIsNaN(number)) {
    return fn(string, "number");
  }
  return fn(`${addNumericSeparator(string)}.${addNumericSeparatorEnd(StringPrototypeSlice(String(number), string.length + 1))}`, "number");
}
function formatBigInt(fn, bigint, numericSeparator) {
  const string = String(bigint);
  if (!numericSeparator) {
    return fn(`${string}n`, "bigint");
  }
  return fn(`${addNumericSeparator(string)}n`, "bigint");
}
function formatPrimitive(fn, value, ctx) {
  if (typeof value === "string") {
    let trailer = "";
    const maxStringLength = ctx.maxStringLength;
    let valueLength = value.length;
    if (valueLength > maxStringLength) {
      const remaining = valueLength - maxStringLength;
      value = StringPrototypeSlice(value, 0, maxStringLength);
      valueLength = value.length;
      trailer = `... ${remaining} more character${remaining > 1 ? "s" : ""}`;
    }
    const indentationLvl = ctx.indentationLvl;
    if (ctx.compact !== true && valueLength > kMinLineLength && valueLength > ctx.breakLength - indentationLvl - 4) {
      return ArrayPrototypeJoin(ArrayPrototypeMap(extractedSplitNewLines(value), (line) => fn(strEscape(line), "string")), ` +
${StringPrototypeRepeat(" ", indentationLvl + 2)}`) + trailer;
    }
    return fn(strEscape(value), "string") + trailer;
  }
  if (typeof value === "number")
    return formatNumber(fn, value, ctx.numericSeparator);
  if (typeof value === "bigint")
    return formatBigInt(fn, value, ctx.numericSeparator);
  if (typeof value === "boolean")
    return fn(`${value}`, "boolean");
  if (typeof value === "undefined")
    return fn("undefined", "undefined");
  return fn(SymbolPrototypeToString(value), "symbol");
}
function formatNamespaceObject(keys, ctx, value, recurseTimes) {
  const output = new Array(keys.length);
  for (let i = 0;i < keys.length; i++) {
    try {
      output[i] = formatProperty(ctx, value, recurseTimes, keys[i], kObjectType);
    } catch (err) {
      assert(isNativeError(err) && err.name === "ReferenceError");
      const tmp = { [keys[i]]: "" };
      output[i] = formatProperty(ctx, tmp, recurseTimes, keys[i], kObjectType);
      const pos = StringPrototypeLastIndexOf(output[i], " ");
      output[i] = StringPrototypeSlice(output[i], 0, pos + 1) + ctx.stylize("<uninitialized>", "special");
    }
  }
  keys.length = 0;
  return output;
}
function formatSpecialArray(ctx, value, recurseTimes, maxLength, output, i) {
  const keys = ObjectKeys(value);
  let index = i;
  for (;i < keys.length && output.length < maxLength; i++) {
    const key = keys[i];
    const tmp = +key;
    if (tmp > 4294967294) {
      break;
    }
    if (`${index}` !== key) {
      if (RegExpPrototypeExec(numberRegExp, key) === null) {
        break;
      }
      const emptyItems = tmp - index;
      const ending = emptyItems > 1 ? "s" : "";
      const message = `<${emptyItems} empty item${ending}>`;
      ArrayPrototypePush.call(output, ctx.stylize(message, "undefined"));
      index = tmp;
      if (output.length === maxLength) {
        break;
      }
    }
    ArrayPrototypePush.call(output, formatProperty(ctx, value, recurseTimes, key, kArrayType));
    index++;
  }
  const remaining = value.length - index;
  if (output.length !== maxLength) {
    if (remaining > 0) {
      const ending = remaining > 1 ? "s" : "";
      const message = `<${remaining} empty item${ending}>`;
      ArrayPrototypePush.call(output, ctx.stylize(message, "undefined"));
    }
  } else if (remaining > 0) {
    ArrayPrototypePush.call(output, remainingText(remaining));
  }
  return output;
}
function hexSlice(buf, start = 0, end) {
  return ArrayPrototypeJoin(ArrayPrototypeMap(buf.slice(start, end), (x) => ("00" + x.toString(16)).slice(-2)), "");
}
function formatArrayBuffer(ctx, value) {
  let buffer;
  try {
    buffer = new Uint8Array(value);
  } catch {
    return [ctx.stylize("(detached)", "special")];
  }
  let str = StringPrototypeTrim(RegExpPrototypeSymbolReplace(/(.{2})/g, hexSlice(buffer, 0, MathMinIntrinsic(ctx.maxArrayLength, buffer.length)), "$1 "));
  const remaining = buffer.length - ctx.maxArrayLength;
  if (remaining > 0)
    str += ` ... ${remaining} more byte${remaining > 1 ? "s" : ""}`;
  return [`${ctx.stylize("[Uint8Contents]", "special")}: <${str}>`];
}
function formatArray(ctx, value, recurseTimes) {
  const valLen = value.length;
  const len = MathMinIntrinsic(MathMax(0, ctx.maxArrayLength), valLen);
  const remaining = valLen - len;
  const output = [];
  for (let i = 0;i < len; i++) {
    if (!ObjectPrototypeHasOwnProperty(value, i)) {
      return formatSpecialArray(ctx, value, recurseTimes, len, output, i);
    }
    ArrayPrototypePush.call(output, formatProperty(ctx, value, recurseTimes, i, kArrayType));
  }
  if (remaining > 0) {
    ArrayPrototypePush.call(output, remainingText(remaining));
  }
  return output;
}
function formatTypedArray(value, length, ctx, _ignored, _recurseTimes) {
  if (Buffer.isBuffer(value)) {
    BufferModule ??= host.requireBuiltin("node:buffer");
    const INSPECT_MAX_BYTES = BufferModule.INSPECT_MAX_BYTES;
    ctx.maxArrayLength = MathMinIntrinsic(ctx.maxArrayLength, INSPECT_MAX_BYTES);
  }
  const maxLength = MathMinIntrinsic(MathMax(0, ctx.maxArrayLength), length);
  const remaining = value.length - maxLength;
  const output = new Array(maxLength);
  const elementFormatter = value.length > 0 && typeof value[0] === "number" ? formatNumber : formatBigInt;
  for (let i = 0;i < maxLength; ++i) {
    output[i] = elementFormatter(ctx.stylize, value[i], ctx.numericSeparator);
  }
  if (remaining > 0) {
    output[maxLength] = remainingText(remaining);
  }
  return output;
}
function formatSet(value, ctx, ignored, recurseTimes) {
  const length = value.size;
  const maxLength = MathMinIntrinsic(MathMax(0, ctx.maxArrayLength), length);
  const remaining = length - maxLength;
  const output = [];
  ctx.indentationLvl += 2;
  let i = 0;
  for (const v of value) {
    if (i >= maxLength)
      break;
    ArrayPrototypePush.call(output, formatValue(ctx, v, recurseTimes));
    i++;
  }
  if (remaining > 0) {
    ArrayPrototypePush.call(output, remainingText(remaining));
  }
  ctx.indentationLvl -= 2;
  return output;
}
function formatMap(value, ctx, ignored, recurseTimes) {
  const length = value.size;
  const maxLength = MathMinIntrinsic(MathMax(0, ctx.maxArrayLength), length);
  const remaining = length - maxLength;
  const output = [];
  ctx.indentationLvl += 2;
  let i = 0;
  for (const { 0: k, 1: v } of value) {
    if (i >= maxLength)
      break;
    ArrayPrototypePush.call(output, `${formatValue(ctx, k, recurseTimes)} => ${formatValue(ctx, v, recurseTimes)}`);
    i++;
  }
  if (remaining > 0) {
    ArrayPrototypePush.call(output, remainingText(remaining));
  }
  ctx.indentationLvl -= 2;
  return output;
}
function formatSetIterInner(ctx, recurseTimes, entries, state) {
  const maxArrayLength = MathMax(ctx.maxArrayLength, 0);
  const maxLength = MathMinIntrinsic(maxArrayLength, entries.length);
  const output = new Array(maxLength);
  ctx.indentationLvl += 2;
  for (let i = 0;i < maxLength; i++) {
    output[i] = formatValue(ctx, entries[i], recurseTimes);
  }
  ctx.indentationLvl -= 2;
  if (state === kWeak && !ctx.sorted) {
    ArrayPrototypeSort(output);
  }
  const remaining = entries.length - maxLength;
  if (remaining > 0) {
    ArrayPrototypePush.call(output, remainingText(remaining));
  }
  return output;
}
function formatMapIterInner(ctx, recurseTimes, entries, state) {
  const maxArrayLength = MathMax(ctx.maxArrayLength, 0);
  const len = entries.length / 2;
  const remaining = len - maxArrayLength;
  const maxLength = MathMinIntrinsic(maxArrayLength, len);
  const output = new Array(maxLength);
  let i = 0;
  ctx.indentationLvl += 2;
  if (state === kWeak) {
    for (;i < maxLength; i++) {
      const pos = i * 2;
      output[i] = `${formatValue(ctx, entries[pos], recurseTimes)} => ${formatValue(ctx, entries[pos + 1], recurseTimes)}`;
    }
    if (!ctx.sorted)
      ArrayPrototypeSort(output);
  } else {
    for (;i < maxLength; i++) {
      const pos = i * 2;
      const res = [formatValue(ctx, entries[pos], recurseTimes), formatValue(ctx, entries[pos + 1], recurseTimes)];
      output[i] = reduceToSingleString(ctx, res, "", ["[", "]"], kArrayExtrasType, recurseTimes);
    }
  }
  ctx.indentationLvl -= 2;
  if (remaining > 0) {
    ArrayPrototypePush.call(output, remainingText(remaining));
  }
  return output;
}
function formatWeakCollection(ctx) {
  return [ctx.stylize("<items unknown>", "special")];
}
function formatWeakSet(ctx, value, recurseTimes) {
  const entries = previewEntries(value);
  return formatSetIterInner(ctx, recurseTimes, entries, kWeak);
}
function formatWeakMap(ctx, value, recurseTimes) {
  const entries = previewEntries(value);
  return formatMapIterInner(ctx, recurseTimes, entries, kWeak);
}
function formatIterator(braces, ctx, value, recurseTimes) {
  const { 0: entries, 1: isKeyValue } = previewEntries(value, true);
  if (isKeyValue) {
    braces[0] = RegExpPrototypeSymbolReplace(/ Iterator] {$/, braces[0], " Entries] {");
    return formatMapIterInner(ctx, recurseTimes, entries, kMapEntries);
  }
  return formatSetIterInner(ctx, recurseTimes, entries, kIterator);
}
function formatPromise(ctx, value, recurseTimes) {
  let output;
  const { 0: state, 1: result } = getPromiseDetails(value);
  if (state === kPending) {
    output = [ctx.stylize("<pending>", "special")];
  } else {
    ctx.indentationLvl += 2;
    const str = formatValue(ctx, result, recurseTimes);
    ctx.indentationLvl -= 2;
    output = [state === kRejected ? `${ctx.stylize("<rejected>", "special")} ${str}` : str];
  }
  return output;
}
function formatExtraProperties(ctx, value, recurseTimes, key, typedArray) {
  ctx.indentationLvl += 2;
  const str = formatValue(ctx, value[key], recurseTimes, typedArray);
  ctx.indentationLvl -= 2;
  const name = ctx.stylize(`[${key}]`, "string");
  return `${name}: ${str}`;
}
function formatProperty(ctx, value, recurseTimes, key, type, desc, original = value) {
  let name, str;
  let extra = " ";
  desc ||= ObjectGetOwnPropertyDescriptor(value, key) || { value: value[key], enumerable: true };
  const descValue = desc.value;
  let descGet;
  if (descValue !== undefined) {
    const diff = ctx.compact !== true || type !== kObjectType ? 2 : 3;
    ctx.indentationLvl += diff;
    str = formatValue(ctx, descValue, recurseTimes);
    if (diff === 3 && ctx.breakLength < getStringWidth(str, ctx.colors)) {
      extra = `
${StringPrototypeRepeat(" ", ctx.indentationLvl)}`;
    }
    ctx.indentationLvl -= diff;
  } else if ((descGet = desc.get) !== undefined) {
    const label = desc.set !== undefined ? "Getter/Setter" : "Getter";
    const s = ctx.stylize;
    const sp = "special";
    if (ctx.getters && (ctx.getters === true || ctx.getters === "get" && desc.set === undefined || ctx.getters === "set" && desc.set !== undefined)) {
      try {
        const tmp = descGet.call(original);
        ctx.indentationLvl += 2;
        if (tmp === null) {
          str = `${s(`[${label}:`, "special")} ${s("null", "null")}${s("]", "special")}`;
        } else if (typeof tmp === "object") {
          str = `${s(`[${label}]`, "special")} ${formatValue(ctx, tmp, recurseTimes)}`;
        } else {
          const primitive = formatPrimitive(s, tmp, ctx);
          str = `${s(`[${label}:`, "special")} ${primitive}${s("]", "special")}`;
        }
        ctx.indentationLvl -= 2;
      } catch (err) {
        const message = `<Inspection threw (${err.message})>`;
        str = `${s(`[${label}:`, "special")} ${message}${s("]", "special")}`;
      }
    } else {
      str = ctx.stylize(`[${label}]`, "special");
    }
  } else if (desc.set !== undefined) {
    str = ctx.stylize("[Setter]", "special");
  } else {
    str = ctx.stylize("undefined", "undefined");
  }
  if (type === kArrayType)
    return str;
  if (typeof key === "symbol") {
    const tmp = RegExpPrototypeSymbolReplace(strEscapeSequencesReplacer, SymbolPrototypeToString(key), escapeFn);
    name = ctx.stylize(tmp, "symbol");
  } else if (RegExpPrototypeExec(keyStrRegExp, key) !== null) {
    name = key === "__proto__" ? "['__proto__']" : ctx.stylize(key, "name");
  } else {
    name = ctx.stylize(strEscape(key), "string");
  }
  if (desc.enumerable === false) {
    name = `[${name}]`;
  }
  return `${name}:${extra}${str}`;
}
function isBelowBreakLength(ctx, output, start, base) {
  let totalLength = output.length + start;
  if (totalLength + output.length > ctx.breakLength)
    return false;
  for (let i = 0;i < output.length; i++) {
    if (ctx.colors) {
      totalLength += StringPrototypeReplaceAll(output[i], /\u001B\[\d\d?m/g, "").length;
    } else {
      totalLength += output[i].length;
    }
    if (totalLength > ctx.breakLength) {
      return false;
    }
  }
  return base === "" || !StringPrototypeIncludes(base, `
`);
}
function reduceToSingleString(ctx, output, base, braces, extrasType, recurseTimes, value) {
  const compact = ctx.compact;
  if (compact !== true) {
    if (typeof compact === "number" && compact >= 1) {
      const entries = output.length;
      if (extrasType === kArrayExtrasType && entries > 6) {
        output = groupArrayElements(ctx, output, value);
      }
      let outputLength;
      if (ctx.currentDepth - recurseTimes < compact && entries === (outputLength = output.length)) {
        const start = outputLength + ctx.indentationLvl + braces[0].length + base.length + 10;
        if (isBelowBreakLength(ctx, output, start, base)) {
          const joinedOutput = ArrayPrototypeJoin(output, ", ");
          if (!StringPrototypeIncludes(joinedOutput, `
`)) {
            return `${base ? `${base} ` : ""}${braces[0]} ${joinedOutput} ${braces[1]}`;
          }
        }
      }
    }
    const indentation = `
${StringPrototypeRepeat(" ", ctx.indentationLvl)}`;
    return `${base ? `${base} ` : ""}${braces[0]}${indentation}  ${ArrayPrototypeJoin(output, `,${indentation}  `)}${indentation}${braces[1]}`;
  }
  if (isBelowBreakLength(ctx, output, 0, base)) {
    return `${braces[0]}${base ? ` ${base}` : ""} ${ArrayPrototypeJoin(output, ", ")} ` + braces[1];
  }
  const indentation = StringPrototypeRepeat(" ", ctx.indentationLvl);
  const ln = base === "" && braces[0].length === 1 ? " " : `${base ? ` ${base}` : ""}
${indentation}  `;
  return `${braces[0]}${ln}${ArrayPrototypeJoin(output, `,
${indentation}  `)} ${braces[1]}`;
}
function returnFalse() {
  return false;
}
function hasBuiltInToString(value) {
  const getFullProxy = false;
  const proxyTarget = getProxyDetails(value, false);
  if (proxyTarget !== undefined) {
    if (proxyTarget === null) {
      return true;
    }
    return hasBuiltInToString(proxyTarget);
  }
  let hasOwnToString = ObjectPrototypeHasOwnProperty;
  let hasOwnToPrimitive = ObjectPrototypeHasOwnProperty;
  if (typeof value.toString !== "function") {
    if (typeof value[SymbolToPrimitive] !== "function") {
      return true;
    } else if (ObjectPrototypeHasOwnProperty(value, SymbolToPrimitive)) {
      return false;
    }
    hasOwnToString = returnFalse;
  } else if (ObjectPrototypeHasOwnProperty(value, "toString")) {
    return false;
  } else if (typeof value[SymbolToPrimitive] !== "function") {
    hasOwnToPrimitive = returnFalse;
  } else if (ObjectPrototypeHasOwnProperty(value, SymbolToPrimitive)) {
    return false;
  }
  let pointer = value;
  do {
    pointer = ObjectGetPrototypeOf(pointer);
  } while (!hasOwnToString(pointer, "toString") && !hasOwnToPrimitive(pointer, SymbolToPrimitive));
  const descriptor = ObjectGetOwnPropertyDescriptor(pointer, "constructor");
  return descriptor !== undefined && typeof descriptor.value === "function" && builtInObjects.has(descriptor.value.name);
}
const firstErrorLine = (error) => StringPrototypeSplit(error.message, `
`, 1)[0];
let CIRCULAR_ERROR_MESSAGE;
function tryStringify(arg) {
  try {
    return JSONStringify(arg);
  } catch (err) {
    if (!CIRCULAR_ERROR_MESSAGE) {
      try {
        const a = {};
        a.a = a;
        JSONStringify(a);
      } catch (circularError) {
        CIRCULAR_ERROR_MESSAGE = firstErrorLine(circularError);
      }
    }
    if (err.name === "TypeError" && firstErrorLine(err) === CIRCULAR_ERROR_MESSAGE) {
      return "[Circular]";
    }
    throw err;
  }
}
function format(...args) {
  return formatWithOptionsInternal(undefined, args);
}
function formatWithOptions(inspectOptions, ...args) {
  validateObject(inspectOptions, "inspectOptions", kValidateObjectAllowArray);
  return formatWithOptionsInternal(inspectOptions, args);
}
function formatNumberNoColor(number, options) {
  return formatNumber(stylizeNoColor, number, options?.numericSeparator ?? inspectDefaultOptions.numericSeparator);
}
function formatBigIntNoColor(bigint, options) {
  return formatBigInt(stylizeNoColor, bigint, options?.numericSeparator ?? inspectDefaultOptions.numericSeparator);
}
function formatWithOptionsInternal(inspectOptions, args) {
  const first = args[0];
  let a = 0;
  let str = "";
  let join = "";
  if (typeof first === "string") {
    if (args.length === 1) {
      return first;
    }
    let tempStr;
    let lastPos = 0;
    for (let i = 0;i < first.length - 1; i++) {
      if (StringPrototypeCharCodeAt(first, i) === 37) {
        const nextChar = StringPrototypeCharCodeAt(first, ++i);
        if (a + 1 !== args.length) {
          switch (nextChar) {
            case 115: {
              const tempArg = args[++a];
              if (typeof tempArg === "number") {
                tempStr = formatNumberNoColor(tempArg, inspectOptions);
              } else if (typeof tempArg === "bigint") {
                tempStr = formatBigIntNoColor(tempArg, inspectOptions);
              } else if (typeof tempArg !== "object" || tempArg === null || !hasBuiltInToString(tempArg)) {
                tempStr = String(tempArg);
              } else {
                tempStr = inspect(tempArg, {
                  ...inspectOptions,
                  compact: 3,
                  colors: false,
                  depth: 0
                });
              }
              break;
            }
            case 106:
              tempStr = tryStringify(args[++a]);
              break;
            case 100: {
              const tempNum = args[++a];
              if (typeof tempNum === "bigint") {
                tempStr = formatBigIntNoColor(tempNum, inspectOptions);
              } else if (typeof tempNum === "symbol") {
                tempStr = "NaN";
              } else {
                tempStr = formatNumberNoColor(Number(tempNum), inspectOptions);
              }
              break;
            }
            case 79:
              tempStr = inspect(args[++a], inspectOptions);
              break;
            case 111:
              tempStr = inspect(args[++a], {
                ...inspectOptions,
                showHidden: true,
                showProxy: true,
                depth: 4
              });
              break;
            case 105: {
              const tempInteger = args[++a];
              if (typeof tempInteger === "bigint") {
                tempStr = formatBigIntNoColor(tempInteger, inspectOptions);
              } else if (typeof tempInteger === "symbol") {
                tempStr = "NaN";
              } else {
                tempStr = formatNumberNoColor(NumberParseInt(tempInteger), inspectOptions);
              }
              break;
            }
            case 102: {
              const tempFloat = args[++a];
              if (typeof tempFloat === "symbol") {
                tempStr = "NaN";
              } else {
                tempStr = formatNumberNoColor(NumberParseFloat(tempFloat), inspectOptions);
              }
              break;
            }
            case 99:
              a += 1;
              tempStr = "";
              break;
            case 37:
              str += StringPrototypeSlice(first, lastPos, i);
              lastPos = i + 1;
              continue;
            default:
              continue;
          }
          if (lastPos !== i - 1) {
            str += StringPrototypeSlice(first, lastPos, i - 1);
          }
          str += tempStr;
          lastPos = i + 1;
        } else if (nextChar === 37) {
          str += StringPrototypeSlice(first, lastPos, i);
          lastPos = i + 1;
        }
      }
    }
    if (lastPos !== 0) {
      a++;
      join = " ";
      if (lastPos < first.length) {
        str += StringPrototypeSlice(first, lastPos);
      }
    }
  }
  while (a < args.length) {
    const value = args[a];
    str += join;
    str += typeof value !== "string" ? inspect(value, inspectOptions) : value;
    join = " ";
    a++;
  }
  return str;
}
const internalGetStringWidth = host.stringWidth;
const kPerCodePointWidthOptions = { __proto__: null, perCodePoint: true, countAnsiEscapeCodes: true };
function getStringWidth(str, removeControlChars = true) {
  if (removeControlChars)
    str = stripVTControlCharacters(str);
  str = StringPrototypeNormalize(str, "NFC");
  return internalGetStringWidth(str, kPerCodePointWidthOptions);
}
let ansi;
function getAnsiRegExp() {
  return ansi ??= new RegExp("[\\u001B\\u009B][[\\]()#;?]*(?:(?:(?:(?:;[-a-zA-Z\\d\\/\\#&.:=?%@~_]+)*|[a-zA-Z\\d]+(?:;[-a-zA-Z\\d\\/\\#&.:=?%@~_]*)*)?(?:\\u0007|\\u001B\\u005C|\\u009C))|(?:(?:\\d{1,4}(?:;\\d{0,4})*)?[\\dA-PR-TZcf-nq-uy=><~]))", "g");
}
function stripVTControlCharacters(str) {
  if (typeof str !== "string")
    throw codes.INVALID_ARG_TYPE("str", "string", str);
  if (StringPrototypeIndexOf(str, "\x1B") === -1 && StringPrototypeIndexOf(str, "") === -1) {
    return str;
  }
  return RegExpPrototypeSymbolReplace(getAnsiRegExp(), str, "");
}
const getOwnNonIndexProperties = host.getOwnNonIndexProperties;
function getPromiseDetails(promise) {
  const state = host.promiseStatus(promise);
  if (state !== 0) {
    return [state === 2 ? kRejected : kFulfilled, host.promiseResult(promise)];
  }
  return [kPending, undefined];
}
function getProxyDetails(proxy, withHandler = true) {
  const isProxy = host.isProxyObject(proxy);
  if (!isProxy)
    return;
  const handler = host.proxyHandler(proxy);
  const target = handler === null ? null : host.proxyTarget(proxy);
  if (withHandler)
    return [target, handler];
  else
    return target;
}
function previewEntries(val, isIterator = false) {
  if (isIterator) {
    const iteratedObject = host.getInternalField(val, 1);
    const kind = host.getInternalField(val, 3);
    const isEntries = kind === 2;
    if (isMap(iteratedObject)) {
      if (isEntries)
        return [ArrayPrototypeFlat(ArrayFrom(iteratedObject)), true];
      else if (kind === 1)
        return [ArrayFrom(MapPrototypeValues(iteratedObject)), false];
      else
        return [ArrayFrom(MapPrototypeKeys(iteratedObject)), false];
    } else if (isSet(iteratedObject)) {
      if (isEntries)
        return [ArrayPrototypeFlat(ArrayFrom(SetPrototypeEntries(iteratedObject))), true];
      else
        return [ArrayFrom(iteratedObject), false];
    } else
      throw new Error("previewEntries(): Invalid iterator received");
  }
  if (isWeakMap(val))
    return [];
  if (isWeakSet(val))
    return [];
  else
    throw new Error("previewEntries(): Invalid object received");
}
function internalGetConstructorName(val) {
  if (!val || typeof val !== "object")
    throw new Error("Invalid object");
  const ctorName = val.constructor?.name;
  if (ctorName)
    return ctorName;
  const str = ObjectPrototypeToString(val);
  const m = StringPrototypeMatch(str, /^\[object ([^\]]+)\]/);
  return m ? m[1] : "Object";
}
return {
  inspect,
  format,
  formatWithOptions,
  getStringWidth,
  stripVTControlCharacters
};

})();
const promisifyModule = (() => {
const kCustomPromisifiedSymbol = Symbol.for("nodejs.util.promisify.custom");
const { validateFunction } = host.internal.validators;
const { kCustomPromisifyArgsSymbol } = host.internal;
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
var promisify = function promisify2(original) {
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
      if (host.isPromise(maybePromise)) {
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

})();
const deprecateModule = (() => {
const { validateString } = host.internal.validators;
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

})();
host.registerInternals({ primordials, colors: colorsModule, inspect: inspectModule, deprecate: deprecateModule });
const types = host.requireBuiltin("node:util/types");
const { promisify } = promisifyModule;
const {
  validateString,
  validateOneOf,
  validateBoolean,
  validateObject,
  validateInteger
} = host.internal.validators;
const { resistStopPropagation, ErrnoException } = host.internal;
const { MIMEType, MIMEParams } = { MIMEType: host.MIMEType, MIMEParams: host.MIMEParams };
const { deprecate } = deprecateModule;
const internalErrorName = host.internalErrorName;
const internalErrorEntries = host.internalErrorEntries;
const parseEnv = host.parseEnv;
const NumberIsSafeInteger = Number.isSafeInteger;
const ObjectKeys = Object.keys;
const ObjectGetOwnPropertyNames = Object.getOwnPropertyNames;
const { uncurryThis, SafeMap } = primordials;
const RegExpPrototypeExec = uncurryThis(RegExp.prototype.exec);
var cjs_exports;
function isBuffer(value) {
  return Buffer.isBuffer(value);
}
function isFunction(value) {
  return typeof value === "function";
}
const { isDeepStrictEqual } = { isDeepStrictEqual: host.jsFunctionIsDeepStrictEqual };
const parseArgs = host.parseArgs;
let utl;
function lazyInspectModule() {
  return utl ??= inspectModule;
}
var debugs = {};
var debugEnvRegex = /^$/;
const NODE_DEBUG = process.env.NODE_DEBUG;
if (NODE_DEBUG) {
  debugEnv = NODE_DEBUG;
  debugEnv = debugEnv.replace(/[|\\{}()[\]^$+?.]/g, "\\$&").replace(/\*/g, ".*").replace(/,/g, "$|^").toUpperCase();
  debugEnvRegex = new RegExp("^" + debugEnv + "$", "i");
}
var debugEnv;
function emitWarningIfNeeded(set) {
  if (set === "HTTP" || set === "HTTP2") {
    process.emitWarning("Setting the NODE_DEBUG environment variable to '" + set.toLowerCase() + "' can expose sensitive data (such as passwords, tokens and authentication headers) in the resulting log.");
  }
}
function debuglog(set) {
  set = set.toUpperCase();
  if (!debugs[set]) {
    if (debugEnvRegex.test(set)) {
      var pid = process.pid;
      emitWarningIfNeeded(set);
      debugs[set] = function() {
        var msg = lazyInspectModule().format.apply(cjs_exports, arguments);
        console.error("%s %d: %s", set, pid, msg);
      };
    } else {
      debugs[set] = function() {};
    }
  }
  return debugs[set];
}
function isBoolean(arg) {
  return typeof arg === "boolean";
}
function isNull(arg) {
  return arg === null;
}
function isNullOrUndefined(arg) {
  return arg == null;
}
function isNumber(arg) {
  return typeof arg === "number";
}
function isString(arg) {
  return typeof arg === "string";
}
function isSymbol(arg) {
  return typeof arg === "symbol";
}
function isUndefined(arg) {
  return arg === undefined;
}
var isRegExp = types.isRegExp;
function isObject(arg) {
  return typeof arg === "object" && arg !== null;
}
var isDate = types.isDate;
var isError = host.jsFunctionIsError;
function isPrimitive(arg) {
  return arg === null || typeof arg === "boolean" || typeof arg === "number" || typeof arg === "string" || typeof arg === "symbol" || typeof arg === "undefined";
}
function pad(n) {
  return n < 10 ? "0" + n.toString(10) : n.toString(10);
}
var months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
function timestamp() {
  var d = new Date;
  var time = [pad(d.getHours()), pad(d.getMinutes()), pad(d.getSeconds())].join(":");
  return [d.getDate(), months[d.getMonth()], time].join(" ");
}
var log = function log2() {
  console.log("%s - %s", timestamp(), lazyInspectModule().format.apply(cjs_exports, arguments));
};
var inherits = function inherits2(ctor, superCtor) {
  if (ctor === undefined || ctor === null) {
    throw codes.INVALID_ARG_TYPE("ctor", "function", ctor);
  }
  if (superCtor === undefined || superCtor === null) {
    throw codes.INVALID_ARG_TYPE("superCtor", "function", superCtor);
  }
  const superCtorPrototype = superCtor.prototype;
  if (superCtorPrototype === undefined) {
    throw codes.INVALID_ARG_TYPE("superCtor.prototype", "object", superCtorPrototype);
  }
  Object.defineProperty(ctor, "super_", {
    __proto__: null,
    value: superCtor,
    writable: true,
    configurable: true
  });
  Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
};
var _extend = function(origin, add) {
  if (!add || !isObject(add))
    return origin;
  var keys = Object.keys(add);
  var i = keys.length;
  while (i--) {
    origin[keys[i]] = add[keys[i]];
  }
  return origin;
};
function callbackifyOnRejected(reason, cb) {
  if (!reason) {
    var newReason = new Error("Promise was rejected with a falsy value");
    newReason.reason = reason;
    newReason.code = "ERR_FALSY_VALUE_REJECTION";
    reason = newReason;
  }
  return cb(reason);
}
function callbackify(original) {
  const { validateFunction } = host.internal.validators;
  validateFunction(original, "original");
  function callbackified(...args) {
    const maybeCb = Array.prototype.pop.call(args);
    validateFunction(maybeCb, "last argument");
    const cb = Function.prototype.bind.call(maybeCb, this);
    original.apply(this, args).then((ret) => process.nextTick(cb, null, ret), (rej) => process.nextTick(callbackifyOnRejected, rej, cb));
  }
  const descriptors = Object.getOwnPropertyDescriptors(original);
  if (typeof descriptors.length.value === "number") {
    descriptors.length.value++;
  }
  if (typeof descriptors.name.value === "string") {
    descriptors.name.value += "Callbackified";
  }
  const propertiesValues = Object.values(descriptors);
  for (let i = 0;i < propertiesValues.length; i++) {
    Object.setPrototypeOf(propertiesValues[i], null);
  }
  Object.defineProperties(callbackified, descriptors);
  return callbackified;
}
var toUSVString = (input) => {
  return (input + "").toWellFormed();
};
const kEscape = "\x1B[";
const kEscapeEnd = "m";
const kDimCode = 2;
const kBoldCode = 1;
const kHexCloseSeq = kEscape + "39" + kEscapeEnd;
const kHexStyleCacheMax = 256;
const hexColorRegExp = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/;
let styleCache;
let hexStyleCache;
let lazyStreamUtils;
let lazyUtilColors;
function getHexStyleCache() {
  hexStyleCache ??= new SafeMap;
  return hexStyleCache;
}
function getStyleCache() {
  if (styleCache === undefined) {
    styleCache = { __proto__: null };
    const colors = lazyInspectModule().inspect.colors;
    for (const key of ObjectGetOwnPropertyNames(colors)) {
      const codes = colors[key];
      if (codes) {
        const openNum = codes[0];
        const closeNum = codes[1];
        styleCache[key] = {
          __proto__: null,
          openSeq: kEscape + openNum + kEscapeEnd,
          closeSeq: kEscape + closeNum + kEscapeEnd,
          keepClose: openNum === kDimCode || openNum === kBoldCode
        };
      }
    }
  }
  return styleCache;
}
function hexToRgb(hex) {
  let hexStr;
  if (hex.length === 4) {
    hexStr = hex[1] + hex[1] + hex[2] + hex[2] + hex[3] + hex[3];
  } else if (hex.length === 7) {
    hexStr = hex.slice(1);
  } else {
    throw codes.OUT_OF_RANGE("hex", "#RGB or #RRGGBB", hex);
  }
  return Buffer.from(hexStr, "hex");
}
function getHexStyle(hex) {
  const cache = getHexStyleCache();
  const cached = cache.get(hex);
  if (cached !== undefined)
    return cached;
  const rgb = hexToRgb(hex);
  const style = {
    __proto__: null,
    openSeq: kEscape + `38;2;${rgb[0]};${rgb[1]};${rgb[2]}` + kEscapeEnd,
    closeSeq: kHexCloseSeq
  };
  if (cache.size >= kHexStyleCacheMax) {
    cache.delete(cache.keys().next().value);
  }
  cache.set(hex, style);
  return style;
}
function replaceCloseCode(str, closeSeq, openSeq, keepClose) {
  const closeLen = closeSeq.length;
  let index = str.indexOf(closeSeq);
  if (index === -1)
    return str;
  let result = "";
  let lastIndex = 0;
  const replacement = keepClose ? closeSeq + openSeq : openSeq;
  do {
    const afterClose = index + closeLen;
    if (afterClose < str.length) {
      result += str.slice(lastIndex, index) + replacement;
      lastIndex = afterClose;
    } else {
      break;
    }
    index = str.indexOf(closeSeq, lastIndex);
  } while (index !== -1);
  return result + str.slice(lastIndex);
}
function styleText(format, text, options) {
  const validateStream = options?.validateStream ?? true;
  const cache = getStyleCache();
  if (!validateStream && typeof format === "string" && typeof text === "string") {
    if (format === "none")
      return text;
    const style = cache[format];
    if (style !== undefined) {
      const processed = replaceCloseCode(text, style.closeSeq, style.openSeq, style.keepClose);
      return style.openSeq + processed + style.closeSeq;
    }
    if (format[0] === "#") {
      let hexStyle = getHexStyleCache().get(format);
      if (hexStyle === undefined && RegExpPrototypeExec(hexColorRegExp, format) !== null) {
        hexStyle = getHexStyle(format);
      }
      if (hexStyle !== undefined) {
        const processed = replaceCloseCode(text, hexStyle.closeSeq, hexStyle.openSeq, false);
        return hexStyle.openSeq + processed + hexStyle.closeSeq;
      }
    }
  }
  validateString(text, "text");
  if (options !== undefined) {
    validateObject(options, "options");
  }
  validateBoolean(validateStream, "options.validateStream");
  let skipColorize;
  if (validateStream) {
    const stream = options?.stream ?? process.stdout;
    lazyStreamUtils ??= host.requireInternal("internal/streams/utils");
    const { isNodeStream, isReadableStream, isWritableStream } = lazyStreamUtils;
    if (!isReadableStream(stream) && !isWritableStream(stream) && !isNodeStream(stream)) {
      throw codes.INVALID_ARG_TYPE("stream", ["ReadableStream", "WritableStream", "Stream"], stream);
    }
    lazyUtilColors ??= colorsModule;
    skipColorize = !lazyUtilColors.shouldColorize(stream);
  }
  const formatArray = host.isJSArray(format) ? format : [format];
  let openCodes = "";
  let closeCodes = "";
  let processedText = text;
  for (const key of formatArray) {
    if (key === "none")
      continue;
    if (typeof key === "string" && key[0] === "#") {
      let hexStyle = getHexStyleCache().get(key);
      if (hexStyle === undefined) {
        if (RegExpPrototypeExec(hexColorRegExp, key) === null) {
          throw codes.INVALID_ARG_VALUE("format", key, "must be a valid hex color (#RGB or #RRGGBB)");
        }
        if (skipColorize)
          continue;
        hexStyle = getHexStyle(key);
      } else if (skipColorize) {
        continue;
      }
      openCodes += hexStyle.openSeq;
      closeCodes = hexStyle.closeSeq + closeCodes;
      processedText = replaceCloseCode(processedText, hexStyle.closeSeq, hexStyle.openSeq, false);
      continue;
    }
    const style = cache[key];
    if (style === undefined) {
      validateOneOf(key, "format", ObjectGetOwnPropertyNames(lazyInspectModule().inspect.colors));
    }
    openCodes += style.openSeq;
    closeCodes = style.closeSeq + closeCodes;
    processedText = replaceCloseCode(processedText, style.closeSeq, style.openSeq, style.keepClose);
  }
  if (skipColorize)
    return text;
  return `${openCodes}${processedText}${closeCodes}`;
}
function getSystemErrorName(err) {
  if (typeof err !== "number")
    throw codes.INVALID_ARG_TYPE("err", "number", err);
  if (err >= 0 || !NumberIsSafeInteger(err))
    throw codes.OUT_OF_RANGE("err", "a negative integer", err);
  return internalErrorName(err);
}
const uvErrorMessages = {
  __proto__: null,
  E2BIG: "argument list too long",
  EACCES: "permission denied",
  EADDRINUSE: "address already in use",
  EADDRNOTAVAIL: "address not available",
  EAFNOSUPPORT: "address family not supported",
  EAGAIN: "resource temporarily unavailable",
  EAI_ADDRFAMILY: "address family not supported",
  EAI_AGAIN: "temporary failure",
  EAI_BADFLAGS: "bad ai_flags value",
  EAI_BADHINTS: "invalid value for hints",
  EAI_CANCELED: "request canceled",
  EAI_FAIL: "permanent failure",
  EAI_FAMILY: "ai_family not supported",
  EAI_MEMORY: "out of memory",
  EAI_NODATA: "no address",
  EAI_NONAME: "unknown node or service",
  EAI_OVERFLOW: "argument buffer overflow",
  EAI_PROTOCOL: "resolved protocol is unknown",
  EAI_SERVICE: "service not available for socket type",
  EAI_SOCKTYPE: "socket type not supported",
  EALREADY: "connection already in progress",
  EBADF: "bad file descriptor",
  EBUSY: "resource busy or locked",
  ECANCELED: "operation canceled",
  ECHARSET: "invalid Unicode character",
  ECONNABORTED: "software caused connection abort",
  ECONNREFUSED: "connection refused",
  ECONNRESET: "connection reset by peer",
  EDESTADDRREQ: "destination address required",
  EEXIST: "file already exists",
  EFAULT: "bad address in system call argument",
  EFBIG: "file too large",
  EHOSTUNREACH: "host is unreachable",
  EINTR: "interrupted system call",
  EINVAL: "invalid argument",
  EIO: "i/o error",
  EISCONN: "socket is already connected",
  EISDIR: "illegal operation on a directory",
  ELOOP: "too many symbolic links encountered",
  EMFILE: "too many open files",
  EMSGSIZE: "message too long",
  ENAMETOOLONG: "name too long",
  ENETDOWN: "network is down",
  ENETUNREACH: "network is unreachable",
  ENFILE: "file table overflow",
  ENOBUFS: "no buffer space available",
  ENODEV: "no such device",
  ENOENT: "no such file or directory",
  ENOMEM: "not enough memory",
  ENONET: "machine is not on the network",
  ENOPROTOOPT: "protocol not available",
  ENOSPC: "no space left on device",
  ENOSYS: "function not implemented",
  ENOTCONN: "socket is not connected",
  ENOTDIR: "not a directory",
  ENOTEMPTY: "directory not empty",
  ENOTSOCK: "socket operation on non-socket",
  ENOTSUP: "operation not supported on socket",
  EOVERFLOW: "value too large for defined data type",
  EPERM: "operation not permitted",
  EPIPE: "broken pipe",
  EPROTO: "protocol error",
  EPROTONOSUPPORT: "protocol not supported",
  EPROTOTYPE: "protocol wrong type for socket",
  ERANGE: "result too large",
  EROFS: "read-only file system",
  ESHUTDOWN: "cannot send after transport endpoint shutdown",
  ESPIPE: "invalid seek",
  ESRCH: "no such process",
  ETIMEDOUT: "connection timed out",
  ETXTBSY: "text file is busy",
  EXDEV: "cross-device link not permitted",
  UNKNOWN: "unknown error",
  EOF: "end of file",
  ENXIO: "no such device or address",
  EMLINK: "too many links",
  EHOSTDOWN: "host is down",
  EREMOTEIO: "remote I/O error",
  ENOTTY: "inappropriate ioctl for device",
  EFTYPE: "inappropriate file type or format",
  EILSEQ: "illegal byte sequence",
  ESOCKTNOSUPPORT: "socket type not supported",
  ENODATA: "no data available",
  EUNATCH: "protocol driver not attached",
  ENOEXEC: "exec format error"
};
let uvErrmap;
function getUvErrmap() {
  if (uvErrmap === undefined) {
    uvErrmap = new Map;
    const flat = internalErrorEntries();
    for (let i = 0;i < flat.length; i += 2) {
      const code = flat[i];
      const name = flat[i + 1];
      if (!uvErrmap.has(code))
        uvErrmap.set(code, [name, uvErrorMessages[name] ?? name]);
    }
  }
  return uvErrmap;
}
function getSystemErrorMap() {
  const copy = new Map;
  for (const [code, entry] of getUvErrmap()) {
    copy.set(code, [entry[0], entry[1]]);
  }
  return copy;
}
function getSystemErrorMessage(err) {
  if (typeof err !== "number")
    throw codes.INVALID_ARG_TYPE("err", "number", err);
  if (err >= 0 || !NumberIsSafeInteger(err))
    throw codes.OUT_OF_RANGE("err", "a negative integer", err);
  const entry = getUvErrmap().get(err);
  return entry !== undefined ? entry[1] : `Unknown system error ${err}`;
}
function _errnoException(err, syscall, original) {
  return new ErrnoException(err, syscall, original);
}
function prepareCallSites(_err, callSites) {
  const result = [];
  for (let i = 0;i < callSites.length; i++) {
    const callSite = callSites[i];
    const columnNumber = (callSite.getColumnNumber() ?? 0) + 1;
    result.push({
      functionName: callSite.getFunctionName() ?? "",
      scriptId: `${callSite.getScriptId()}`,
      scriptName: callSite.getFileName() ?? "",
      lineNumber: callSite.getLineNumber() ?? 0,
      columnNumber,
      column: columnNumber
    });
  }
  return result;
}
function validateSourceMapOption(options) {
  const { sourceMap } = options;
  if (sourceMap !== undefined) {
    validateBoolean(sourceMap, "options.sourceMap");
  }
}
function getCallSites(frameCount = 10, options) {
  if (options === undefined) {
    if (typeof frameCount === "object" && frameCount !== null) {
      options = frameCount;
      validateObject(options, "options");
      validateSourceMapOption(options);
      frameCount = 10;
    } else {
      options = {};
    }
  } else {
    validateObject(options, "options");
    validateSourceMapOption(options);
  }
  validateInteger(frameCount, "frameCount", 1, 200);
  const target = {};
  const savedPrepareStackTrace = Error.prepareStackTrace;
  const savedStackTraceLimit = Error.stackTraceLimit;
  try {
    Error.prepareStackTrace = prepareCallSites;
    try {
      Error.stackTraceLimit = frameCount;
    } catch {}
    Error.captureStackTrace(target, getCallSites);
    return target.stack;
  } finally {
    Error.prepareStackTrace = savedPrepareStackTrace;
    try {
      Error.stackTraceLimit = savedStackTraceLimit;
    } catch {}
  }
}
let lazySignals;
function getSignals() {
  lazySignals ??= host.requireBuiltin("node:os").constants.signals;
  return lazySignals;
}
function convertProcessSignalToExitCode(signalCode) {
  const signals = getSignals();
  validateOneOf(signalCode, "signalCode", ObjectKeys(signals));
  return 128 + signals[signalCode];
}
let lazyAbortedRegistry;
function onAbortedCallback(promise) {
  lazyAbortedRegistry.unregister(promise);
  host.resolvePromise(promise, undefined);
}
function aborted(signal, resource) {
  if (!isObjectIntrinsic(signal) || !(signal instanceof AbortSignal)) {
    throw codes.INVALID_ARG_TYPE("signal", "AbortSignal", signal);
  }
  if (!isObjectIntrinsic(resource)) {
    throw codes.INVALID_ARG_TYPE("resource", "object", resource);
  }
  if (signal.aborted) {
    return Promise.resolve();
  }
  const promise = host.newPromise();
  const listener = onAbortedCallback.bind(undefined, promise);
  signal.addEventListener("abort", listener, resistStopPropagation({ __proto__: null, once: true }));
  if (!lazyAbortedRegistry) {
    lazyAbortedRegistry = new FinalizationRegistry(({ ref, listener }) => {
      const signal = ref.deref();
      if (signal)
        signal.removeEventListener("abort", listener);
    });
  }
  lazyAbortedRegistry.register(resource, {
    ref: new WeakRef(signal),
    listener
  }, promise);
  return promise;
}
function setTraceSigInt(enable) {
  validateBoolean(enable, "enable");
  if (!Bun.isMainThread) {
    throw codes.WORKER_UNSUPPORTED_OPERATION("Calling util.setTraceSigInt is not supported in workers");
  }
}
cjs_exports = {
  _errnoException,
  _extend,
  callbackify,
  convertProcessSignalToExitCode,
  debug: debuglog,
  debuglog,
  deprecate,
  get format() {
    return lazyInspectModule().format;
  },
  set format(value) {
    Object.defineProperty(cjs_exports, "format", { value, writable: true, enumerable: true, configurable: true });
  },
  styleText,
  get formatWithOptions() {
    return lazyInspectModule().formatWithOptions;
  },
  set formatWithOptions(value) {
    Object.defineProperty(cjs_exports, "formatWithOptions", {
      value,
      writable: true,
      enumerable: true,
      configurable: true
    });
  },
  getCallSites,
  getSystemErrorMap,
  getSystemErrorName,
  getSystemErrorMessage,
  inherits,
  get inspect() {
    return lazyInspectModule().inspect;
  },
  set inspect(value) {
    Object.defineProperty(cjs_exports, "inspect", { value, writable: true, enumerable: true, configurable: true });
  },
  isDeepStrictEqual,
  promisify,
  setTraceSigInt,
  get stripVTControlCharacters() {
    return lazyInspectModule().stripVTControlCharacters;
  },
  set stripVTControlCharacters(value) {
    Object.defineProperty(cjs_exports, "stripVTControlCharacters", {
      value,
      writable: true,
      enumerable: true,
      configurable: true
    });
  },
  toUSVString,
  aborted,
  types,
  parseEnv,
  parseArgs,
  TextDecoder,
  TextEncoder,
  MIMEType,
  MIMEParams,
  isArray: ArrayIsArrayIntrinsic,
  isBoolean,
  isBuffer,
  isNull,
  isNullOrUndefined,
  isNumber,
  isString,
  isSymbol,
  isUndefined,
  isRegExp,
  isObject,
  isDate,
  isError,
  isFunction,
  isPrimitive,
  log
};
return cjs_exports;

})
