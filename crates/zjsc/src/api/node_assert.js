// GERADO por `scripts/gen-node-assert.js` a partir do `assert.ts` e dos internos que ele usa, do bun 1.4.2. Não editar à mão.
(function (host) {
  "use strict";

  const { codes } = host.internal;
const myersDiffModule = (() => {
const colors = host.utilInternal("colors");
var Operation;
((Operation) => {
  Operation[Operation["Insert"] = 0] = "Insert";
  Operation[Operation["Delete"] = 1] = "Delete";
  Operation[Operation["Equal"] = 2] = "Equal";
})(Operation ||= {});
const native = { myersDiff: host.myersDiff, printSimpleMyersDiff: host.printSimpleMyersDiff, printMyersDiff: host.printMyersDiff };
function printSimpleMyersDiff(actual, expected) {
  return native.printSimpleMyersDiff(actual, expected, colors);
}
function printMyersDiff(actual, expected, checkCommaDisparity) {
  return native.printMyersDiff(actual, expected, checkCommaDisparity, colors);
}
return { myersDiff: native.myersDiff, printMyersDiff, printSimpleMyersDiff };

})();
const assertionErrorModule = (() => {
const { SafeSet } = host.utilInternal("primordials");
const { inspect } = host.utilInternal("inspect");
const colors = host.utilInternal("colors");
const { validateObject } = host.internal.validators;
const { printMyersDiff, printSimpleMyersDiff } = myersDiffModule;
const ErrorCaptureStackTrace = Error.captureStackTrace;
const ObjectAssign = Object.assign;
const ObjectDefineProperty = Object.defineProperty;
const ObjectGetPrototypeOf = Object.getPrototypeOf;
const ObjectPrototypeHasOwnProperty = Object.prototype.hasOwnProperty;
const ArrayPrototypeJoin = Array.prototype.join;
const ArrayPrototypePop = Array.prototype.pop;
const ArrayPrototypeSlice = Array.prototype.slice;
const StringPrototypeRepeat = String.prototype.repeat;
const StringPrototypeSlice = String.prototype.slice;
const StringPrototypeSplit = String.prototype.split;
const kReadableOperator = {
  deepStrictEqual: "Expected values to be strictly deep-equal:",
  strictEqual: "Expected values to be strictly equal:",
  strictEqualObject: 'Expected "actual" to be reference-equal to "expected":',
  deepEqual: "Expected values to be loosely deep-equal:",
  notDeepStrictEqual: 'Expected "actual" not to be strictly deep-equal to:',
  notStrictEqual: 'Expected "actual" to be strictly unequal to:',
  notStrictEqualObject: 'Expected "actual" not to be reference-equal to "expected":',
  notDeepEqual: 'Expected "actual" not to be loosely deep-equal to:',
  notIdentical: "Values have same structure but are not reference-equal:",
  notDeepEqualUnequal: "Expected values not to be loosely deep-equal:",
  partialDeepStrictEqual: "Expected values to be partially and strictly deep-equal:"
};
const kMaxShortStringLength = 12;
const kMaxLongStringLength = 512;
const kMethodsWithCustomMessageDiff = new SafeSet(["deepStrictEqual", "strictEqual", "partialDeepStrictEqual"]);
function copyError(source) {
  const target = ObjectAssign({ __proto__: ObjectGetPrototypeOf(source) }, source);
  ObjectDefineProperty(target, "message", {
    __proto__: null,
    value: source.message
  });
  if (ObjectPrototypeHasOwnProperty.call(source, "cause")) {
    let { cause } = source;
    if (Error.isError(cause)) {
      cause = copyError(cause);
    }
    ObjectDefineProperty(target, "cause", { __proto__: null, value: cause });
  }
  return target;
}
function inspectValue(val) {
  return inspect(val, {
    compact: false,
    customInspect: false,
    depth: 1000,
    maxArrayLength: 1 / 0,
    showHidden: false,
    showProxy: false,
    sorted: true,
    getters: true
  });
}
function getErrorMessage(operator, message) {
  return message || kReadableOperator[operator];
}
function checkOperator(actual, expected, operator) {
  if (operator === "strictEqual" && (typeof actual === "object" && actual !== null && typeof expected === "object" && expected !== null || typeof actual === "function" && typeof expected === "function")) {
    operator = "strictEqualObject";
  }
  return operator;
}
function getColoredMyersDiff(actual, expected) {
  const header = `${colors.green}actual${colors.white} ${colors.red}expected${colors.white}`;
  const skipped = false;
  let message = printSimpleMyersDiff(actual, expected);
  if (false) {}
  return { message, header, skipped: false };
}
function getStackedDiff(actual, expected) {
  const isStringComparison = typeof actual === "string" && typeof expected === "string";
  let message = `
${colors.green}+${colors.white} ${actual}
${colors.red}- ${colors.white}${expected}`;
  const stringsLen = actual.length + expected.length;
  const maxTerminalLength = process.stderr.isTTY ? process.stderr.columns : 80;
  const showIndicator = isStringComparison && stringsLen <= maxTerminalLength;
  if (showIndicator) {
    let indicatorIdx = -1;
    for (let i = 0;i < actual.length; i++) {
      if (actual[i] !== expected[i]) {
        if (i >= 3) {
          indicatorIdx = i;
        }
        break;
      }
    }
    if (indicatorIdx !== -1) {
      message += `
${StringPrototypeRepeat.call(" ", indicatorIdx + 2)}^`;
    }
  }
  return { message };
}
function getSimpleDiff(originalActual, actual, originalExpected, expected) {
  let stringsLen = actual.length + expected.length;
  if (typeof originalActual === "string") {
    stringsLen -= 2;
  }
  if (typeof originalExpected === "string") {
    stringsLen -= 2;
  }
  if (stringsLen <= 12 && (originalActual !== 0 || originalExpected !== 0)) {
    return { message: `${actual} !== ${expected}`, header: "" };
  }
  const isStringComparison = typeof originalActual === "string" && typeof originalExpected === "string";
  if (isStringComparison && colors.hasColors) {
    try {
      return getColoredMyersDiff(actual, expected);
    } catch {
      return getStackedDiff(actual, expected);
    }
  }
  return getStackedDiff(actual, expected);
}
function isSimpleDiff(actual, inspectedActual, expected, inspectedExpected) {
  if (inspectedActual.length > 1 || inspectedExpected.length > 1) {
    return false;
  }
  return typeof actual !== "object" || actual === null || typeof expected !== "object" || expected === null;
}
function createErrDiff(actual, expected, operator, customMessage, diffType = "simple") {
  operator = checkOperator(actual, expected, operator);
  let skipped = false;
  let message = "";
  const inspectedActual = inspectValue(actual);
  const inspectedExpected = inspectValue(expected);
  const inspectedSplitActual = StringPrototypeSplit.call(inspectedActual, `
`);
  const inspectedSplitExpected = StringPrototypeSplit.call(inspectedExpected, `
`);
  const showSimpleDiff = isSimpleDiff(actual, inspectedSplitActual, expected, inspectedSplitExpected);
  let header = `${colors.green}+ actual${colors.white} ${colors.red}- expected${colors.white}`;
  if (showSimpleDiff) {
    const simpleDiff = getSimpleDiff(actual, inspectedSplitActual[0], expected, inspectedSplitExpected[0]);
    message = simpleDiff.message;
    const simpleHeader = simpleDiff.header;
    if (typeof simpleHeader !== "undefined") {
      header = simpleHeader;
    }
    if (simpleDiff.skipped) {
      skipped = true;
    }
  } else if (inspectedActual === inspectedExpected) {
    operator = "notIdentical";
    if (inspectedSplitActual.length > 50 && diffType !== "full") {
      message = `${ArrayPrototypeJoin.call(ArrayPrototypeSlice.call(inspectedSplitActual, 0, 50), `
`)}
...}`;
      skipped = true;
    } else {
      message = ArrayPrototypeJoin.call(inspectedSplitActual, `
`);
    }
    header = "";
  } else {
    const checkCommaDisparity = actual != null && typeof actual === "object";
    let myersDiffMessage;
    try {
      myersDiffMessage = printMyersDiff(inspectedActual, inspectedExpected, checkCommaDisparity);
    } catch {
      myersDiffMessage = undefined;
    }
    if (myersDiffMessage === undefined) {
      message = `${ArrayPrototypeJoin.call(ArrayPrototypeSlice.call(inspectedSplitActual, 0, 50), `
`)}
...`;
      header = "";
      skipped = true;
    } else {
      message = myersDiffMessage.message;
      if (myersDiffMessage.skipped) {
        skipped = true;
      }
    }
  }
  const headerMessage = `${getErrorMessage(operator, customMessage)}
${header}`;
  const skippedMessage = skipped ? `
... Skipped lines` : "";
  return `${headerMessage}${skippedMessage}
${message}
`;
}
function addEllipsis(string) {
  const lines = StringPrototypeSplit.call(string, `
`, 11);
  if (lines.length > 10) {
    lines.length = 10;
    return `${ArrayPrototypeJoin.call(lines, `
`)}
...`;
  } else if (string.length > 512) {
    return `${StringPrototypeSlice.call(string, 512)}...`;
  }
  return string;
}

class AssertionError extends Error {
  generatedMessage;
  actual;
  expected;
  operator;
  constructor(options) {
    validateObject(options, "options");
    const {
      message,
      operator,
      stackStartFn,
      details,
      stackStartFunction,
      diff = "simple"
    } = options;
    let { actual, expected } = options;
    const limit = Error.stackTraceLimit;
    Error.stackTraceLimit = 0;
    if (message != null) {
      if (kMethodsWithCustomMessageDiff.has(operator)) {
        super(createErrDiff(actual, expected, operator, message, diff));
      } else {
        super(String(message));
      }
    } else {
      colors.refresh();
      if (typeof actual === "object" && actual !== null && typeof expected === "object" && expected !== null && "stack" in actual && actual instanceof Error && "stack" in expected && expected instanceof Error) {
        actual = copyError(actual);
        expected = copyError(expected);
      }
      if (kMethodsWithCustomMessageDiff.has(operator)) {
        super(createErrDiff(actual, expected, operator, message, diff));
      } else if (operator === "notDeepStrictEqual" || operator === "notStrictEqual") {
        let base = kReadableOperator[operator];
        const res = StringPrototypeSplit.call(inspectValue(actual), `
`);
        if (operator === "notStrictEqual" && (typeof actual === "object" && actual !== null || typeof actual === "function")) {
          base = kReadableOperator.notStrictEqualObject;
        }
        const resLength = res.length;
        if (resLength > 50 && diff !== "full") {
          res[46] = `${colors.blue}...${colors.white}`;
          while (res.length > 47) {
            ArrayPrototypePop.call(res);
          }
        }
        if (res.length === 1) {
          super(`${base}${res[0].length > 5 ? `

` : " "}${res[0]}`);
        } else {
          super(`${base}

${ArrayPrototypeJoin.call(res, `
`)}
`);
        }
      } else {
        let res = inspectValue(actual);
        let other = inspectValue(expected);
        const knownOperator = kReadableOperator[operator];
        if (operator === "notDeepEqual" && res === other) {
          res = `${knownOperator}

${res}`;
          if (res.length > 1024 && diff !== "full") {
            res = `${StringPrototypeSlice.call(res, 0, 1021)}...`;
          }
          super(res);
        } else {
          if (res.length > 512 && diff !== "full") {
            res = `${StringPrototypeSlice.call(res, 0, 509)}...`;
          }
          if (other.length > 512 && diff !== "full") {
            other = `${StringPrototypeSlice.call(other, 0, 509)}...`;
          }
          if (operator === "deepEqual") {
            res = `${knownOperator}

${res}

should loosely deep-equal

`;
          } else {
            const newOp = kReadableOperator[`${operator}Unequal`];
            if (newOp) {
              res = `${newOp}

${res}

should not loosely deep-equal

`;
            } else {
              other = ` ${operator} ${other}`;
            }
          }
          super(`${res}${other}`);
        }
      }
    }
    Error.stackTraceLimit = limit;
    this.generatedMessage = !message;
    ObjectDefineProperty(this, "name", {
      __proto__: null,
      value: "AssertionError [ERR_ASSERTION]",
      enumerable: false,
      writable: true,
      configurable: true
    });
    this.code = "ERR_ASSERTION";
    if (details) {
      for (let i = 0;i < details.length; i++) {
        this["message " + i] = details[i].message;
        this["actual " + i] = details[i].actual;
        this["expected " + i] = details[i].expected;
        this["operator " + i] = details[i].operator;
        this["stack trace " + i] = details[i].stack;
      }
    } else {
      this.actual = actual;
      this.expected = expected;
      this.operator = operator;
    }
    ErrorCaptureStackTrace(this, stackStartFn || stackStartFunction);
    {
      const s = this.stack;
      if ((s) == null || typeof s === "string" && s.indexOf(`
    at `) === -1) {
        ErrorCaptureStackTrace(this, AssertionError);
      }
    }
    this.stack;
    this.name = "AssertionError";
    this.diff = diff;
  }
  toString() {
    return `${this.name} [${this.code}]: ${this.message}`;
  }
  [inspect.custom](recurseTimes, ctx) {
    const tmpActual = this.actual;
    const tmpExpected = this.expected;
    if (typeof this.actual === "string") {
      this.actual = addEllipsis(this.actual);
    }
    if (typeof this.expected === "string") {
      this.expected = addEllipsis(this.expected);
    }
    const result = inspect(this, {
      ...ctx,
      customInspect: false,
      depth: 0
    });
    this.actual = tmpActual;
    this.expected = tmpExpected;
    return result;
  }
}
return AssertionError;

})();
const assertUtilsModule = (() => {
var AssertionError;
function loadAssertionError() {
  if (AssertionError === undefined) {
    AssertionError = assertionErrorModule;
  }
}
function innerOk(fn, argLen, value, message) {
  if (!value) {
    let generatedMessage = false;
    if (argLen === 0) {
      generatedMessage = true;
      message = "No value argument passed to `assert.ok()`";
    } else if (message == null) {
      generatedMessage = true;
      message = undefined;
    } else if (Error.isError(message)) {
      throw message;
    }
    if (AssertionError === undefined)
      loadAssertionError();
    const err = new AssertionError({
      actual: value,
      expected: true,
      message,
      operator: "==",
      stackStartFn: fn
    });
    err.generatedMessage = generatedMessage;
    throw err;
  }
}

return { innerOk };

})();
const callTrackerModule = (() => {
const { SafeSet, SafeWeakMap } = host.utilInternal("primordials");
const AssertionError = assertionErrorModule;
const { validateUint32 } = host.internal.validators;
const ObjectFreeze = Object.freeze;
const ArrayPrototypePush = Array.prototype.push;
const ArrayPrototypeSlice = Array.prototype.slice;
const noop = () => {};

class CallTrackerContext {
  #expected;
  #calls;
  #name;
  #stackTrace;
  constructor({ expected, stackTrace, name }) {
    this.#calls = [];
    this.#expected = expected;
    this.#stackTrace = stackTrace;
    this.#name = name;
  }
  track(thisArg, args) {
    const argsClone = ObjectFreeze(ArrayPrototypeSlice.call(args));
    ArrayPrototypePush.call(this.#calls, ObjectFreeze({ thisArg, arguments: argsClone }));
  }
  get delta() {
    return this.#calls.length - this.#expected;
  }
  reset() {
    this.#calls = [];
  }
  getCalls() {
    return ObjectFreeze(ArrayPrototypeSlice.call(this.#calls));
  }
  report() {
    if (this.delta !== 0) {
      const message = `Expected the ${this.#name} function to be executed ${this.#expected} time(s) but was executed ${this.#calls.length} time(s).`;
      return {
        message,
        actual: this.#calls.length,
        expected: this.#expected,
        operator: this.#name,
        stack: this.#stackTrace
      };
    }
  }
}

class CallTracker {
  #callChecks = new SafeSet;
  #trackedFunctions = new SafeWeakMap;
  #getTrackedFunction(tracked) {
    if (!this.#trackedFunctions.has(tracked)) {
      throw codes.INVALID_ARG_VALUE("tracked", tracked, "is not a tracked function");
    }
    return this.#trackedFunctions.get(tracked);
  }
  reset(tracked) {
    if (tracked === undefined) {
      this.#callChecks.forEach((check) => check.reset());
      return;
    }
    this.#getTrackedFunction(tracked).reset();
  }
  getCalls(tracked) {
    return this.#getTrackedFunction(tracked).getCalls();
  }
  calls(fn, expected = 1) {
    if (process._exiting)
      throw codes.UNAVAILABLE_DURING_EXIT();
    if (typeof fn === "number") {
      expected = fn;
      fn = noop;
    } else if (fn === undefined) {
      fn = noop;
    }
    validateUint32(expected, "expected", true);
    const context = new CallTrackerContext({
      expected,
      stackTrace: new Error,
      name: fn.name || "calls"
    });
    const tracked = new Proxy(fn, {
      __proto__: null,
      apply(fn, thisArg, argList) {
        context.track(thisArg, argList);
        return fn.apply(thisArg, argList);
      }
    });
    this.#callChecks.add(context);
    this.#trackedFunctions.set(tracked, context);
    return tracked;
  }
  report() {
    const errors = [];
    for (const context of this.#callChecks) {
      const message = context.report();
      if (message !== undefined) {
        ArrayPrototypePush.call(errors, message);
      }
    }
    return errors;
  }
  verify() {
    const errors = this.report();
    if (errors.length === 0) {
      return;
    }
    const message = errors.length === 1 ? errors[0].message : "Functions were not called the expected number of times";
    throw new AssertionError({
      message,
      details: errors
    });
  }
}
return CallTracker;

})();
const { isPromise, isRegExp } = host.requireBuiltin("node:util/types");
const { innerOk } = assertUtilsModule;
const { validateFunction, validateOneOf } = host.internal.validators;
const ArrayPrototypeIndexOf = Array.prototype.indexOf;
const ArrayPrototypeJoin = Array.prototype.join;
const ArrayPrototypePush = Array.prototype.push;
const ArrayPrototypeSlice = Array.prototype.slice;
const nodePartialDeepStrictEqual = host.jsFunctionPartialDeepStrictEqual;
const NumberIsNaN = Number.isNaN;
const ObjectAssign = Object.assign;
const ObjectDefineProperty = Object.defineProperty;
const ObjectGetOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
const ObjectIs = Object.is;
const ObjectKeys = Object.keys;
const ObjectPrototypeIsPrototypeOf = Object.prototype.isPrototypeOf;
const RegExpPrototypeExec = RegExp.prototype.exec;
const StringPrototypeIndexOf = String.prototype.indexOf;
const StringPrototypeSlice = String.prototype.slice;
const StringPrototypeSplit = String.prototype.split;
const kOptions = Symbol("options");
const { isDeepStrictEqual } = { isDeepStrictEqual: host.jsFunctionIsDeepStrictEqual };
function isDeepEqual(a, b) {
  return Bun.deepEquals(a, b, false);
}
var _inspect;
function lazyInspect() {
  if (_inspect === undefined) {
    _inspect = host.utilInternal("inspect").inspect;
  }
  return _inspect;
}
var AssertionError;
function loadAssertionError() {
  if (AssertionError === undefined) {
    AssertionError = assertionErrorModule;
  }
}
let warned = false;
const assert = ok;
const NO_EXCEPTION_SENTINEL = {};
function Assert(options) {
  if (!new.target) {
    throw codes.CONSTRUCT_CALL_REQUIRED("Class constructor Assert cannot be invoked without `new`");
  }
  options = ObjectAssign({ __proto__: null, strict: true, skipPrototype: false }, options);
  const { diff } = options;
  if (diff !== undefined) {
    validateOneOf(diff, "options.diff", ["simple", "full"]);
  }
  loadAssertionError();
  this.AssertionError = AssertionError;
  ObjectDefineProperty(this, kOptions, {
    __proto__: null,
    value: options,
    enumerable: false,
    configurable: false,
    writable: false
  });
  if (options.strict) {
    this.equal = this.strictEqual;
    this.deepEqual = this.deepStrictEqual;
    this.notEqual = this.notStrictEqual;
    this.notDeepEqual = this.notDeepStrictEqual;
  }
}
Assert.prototype = {};
ObjectDefineProperty(Assert.prototype, "constructor", {
  __proto__: null,
  value: Assert,
  writable: true,
  enumerable: false,
  configurable: true
});
function innerFail(obj) {
  const objMessage = obj.message;
  if (objMessage instanceof Error)
    throw objMessage;
  if (AssertionError === undefined)
    loadAssertionError();
  throw new AssertionError(obj);
}
function fail(actual, expected, message, operator, stackStartFn) {
  const argsLen = arguments.length;
  let internalMessage = false;
  if (actual == null && argsLen <= 1) {
    internalMessage = true;
    message = "Failed";
  } else if (argsLen === 1) {
    message = actual;
    actual = undefined;
  } else {
    if (warned === false) {
      warned = true;
      process.emitWarning("assert.fail() with more than one argument is deprecated. Please use assert.strictEqual() instead or only pass a message.", "DeprecationWarning", "DEP0094");
    }
    if (argsLen === 2)
      operator = "!=";
  }
  if (message instanceof Error)
    throw message;
  const errArgs = {
    actual,
    expected,
    operator: operator === undefined ? "fail" : operator,
    stackStartFn: stackStartFn || fail,
    message,
    diff: this?.[kOptions]?.diff
  };
  if (AssertionError === undefined)
    loadAssertionError();
  const err = new AssertionError(errArgs);
  if (internalMessage) {
    err.generatedMessage = true;
  }
  throw err;
}
Assert.prototype.fail = fail;
Object.defineProperty(assert, "AssertionError", {
  get() {
    loadAssertionError();
    return AssertionError;
  },
  set(value) {
    Object.defineProperty(this, "AssertionError", { value, writable: true, enumerable: true, configurable: true });
  },
  configurable: true,
  enumerable: true
});
function ok(...args) {
  innerOk(ok, args.length, ...args);
}
Assert.prototype.ok = function ok2(...args) {
  innerOk(ok2, args.length, ...args);
};
Assert.prototype.equal = function equal(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (actual != expected && (!NumberIsNaN(actual) || !NumberIsNaN(expected))) {
    innerFail({
      actual,
      expected,
      message,
      operator: "==",
      stackStartFn: equal,
      diff: this?.[kOptions]?.diff
    });
  }
};
Assert.prototype.notEqual = function notEqual(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (actual == expected || NumberIsNaN(actual) && NumberIsNaN(expected)) {
    innerFail({
      actual,
      expected,
      message,
      operator: "!=",
      stackStartFn: notEqual,
      diff: this?.[kOptions]?.diff
    });
  }
};
Assert.prototype.deepEqual = function deepEqual(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (!isDeepEqual(actual, expected)) {
    innerFail({
      actual,
      expected,
      message,
      operator: "deepEqual",
      stackStartFn: deepEqual,
      diff: this?.[kOptions]?.diff
    });
  }
};
Assert.prototype.notDeepEqual = function notDeepEqual(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (isDeepEqual(actual, expected)) {
    innerFail({
      actual,
      expected,
      message,
      operator: "notDeepEqual",
      stackStartFn: notDeepEqual,
      diff: this?.[kOptions]?.diff
    });
  }
};
Assert.prototype.deepStrictEqual = function deepStrictEqual(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (!isDeepStrictEqual(actual, expected, this?.[kOptions]?.skipPrototype)) {
    innerFail({
      actual,
      expected,
      message,
      operator: "deepStrictEqual",
      stackStartFn: deepStrictEqual,
      diff: this?.[kOptions]?.diff
    });
  }
};
Assert.prototype.notDeepStrictEqual = notDeepStrictEqual;
function notDeepStrictEqual(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (isDeepStrictEqual(actual, expected, this?.[kOptions]?.skipPrototype)) {
    innerFail({
      actual,
      expected,
      message,
      operator: "notDeepStrictEqual",
      stackStartFn: notDeepStrictEqual,
      diff: this?.[kOptions]?.diff
    });
  }
}
Assert.prototype.strictEqual = function strictEqual(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (!ObjectIs(actual, expected)) {
    innerFail({
      actual,
      expected,
      message,
      operator: "strictEqual",
      stackStartFn: strictEqual,
      diff: this?.[kOptions]?.diff
    });
  }
};
Assert.prototype.notStrictEqual = function notStrictEqual(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (ObjectIs(actual, expected)) {
    innerFail({
      actual,
      expected,
      message,
      operator: "notStrictEqual",
      stackStartFn: notStrictEqual,
      diff: this?.[kOptions]?.diff
    });
  }
};
Assert.prototype.partialDeepStrictEqual = function partialDeepStrictEqual(actual, expected, message) {
  if (arguments.length < 2) {
    throw codes.MISSING_ARGS("actual", "expected");
  }
  if (!nodePartialDeepStrictEqual(actual, expected)) {
    innerFail({
      actual,
      expected,
      message,
      operator: "partialDeepStrictEqual",
      stackStartFn: partialDeepStrictEqual,
      diff: this?.[kOptions]?.diff
    });
  }
};

class Comparison {
  constructor(obj, keys, actual) {
    for (const key of keys) {
      if (key in obj) {
        if (actual !== undefined && typeof actual[key] === "string" && isRegExp(obj[key]) && RegExpPrototypeExec.call(obj[key], actual[key]) !== null) {
          this[key] = actual[key];
        } else {
          this[key] = obj[key];
        }
      }
    }
  }
}
function compareExceptionKey(actual, expected, key, message, keys, fn) {
  if (!(key in actual) || !isDeepStrictEqual(actual[key], expected[key])) {
    if (!message) {
      const a = new Comparison(actual, keys);
      const b = new Comparison(expected, keys, actual);
      if (AssertionError === undefined)
        loadAssertionError();
      const err = new AssertionError({
        actual: a,
        expected: b,
        operator: "deepStrictEqual",
        stackStartFn: fn
      });
      err.actual = actual;
      err.expected = expected;
      err.operator = fn.name;
      throw err;
    }
    innerFail({
      actual,
      expected,
      message,
      operator: fn.name,
      stackStartFn: fn
    });
  }
}
function expectedException(actual, expected, message, fn) {
  let generatedMessage = false;
  let throwError = false;
  if (typeof expected !== "function") {
    if (isRegExp(expected)) {
      const str = String(actual);
      if (RegExpPrototypeExec.call(expected, str) !== null)
        return;
      const inspect = lazyInspect();
      if (!message) {
        generatedMessage = true;
        message = `The input did not match the regular expression ${inspect(expected)}. Input:

${inspect(str)}
`;
      }
      throwError = true;
    } else if (typeof actual !== "object" || actual === null) {
      if (AssertionError === undefined)
        loadAssertionError();
      const err = new AssertionError({
        actual,
        expected,
        message,
        operator: "deepStrictEqual",
        stackStartFn: fn
      });
      err.operator = fn.name;
      throw err;
    } else {
      const keys = ObjectKeys(expected);
      if (expected instanceof Error) {
        ArrayPrototypePush.call(keys, "name", "message");
      } else if (keys.length === 0) {
        throw codes.INVALID_ARG_VALUE("error", expected, "may not be an empty object");
      }
      for (const key of keys) {
        if (typeof actual[key] === "string" && isRegExp(expected[key]) && RegExpPrototypeExec.call(expected[key], actual[key]) !== null) {
          continue;
        }
        compareExceptionKey(actual, expected, key, message, keys, fn);
      }
      return;
    }
  } else if (expected.prototype !== undefined && actual instanceof expected) {
    return;
  } else if (ObjectPrototypeIsPrototypeOf.call(Error, expected)) {
    if (!message) {
      generatedMessage = true;
      message = `The error is expected to be an instance of "${expected.name}". Received `;
      if (Error.isError(actual)) {
        const name = actual.constructor?.name || actual.name;
        if (expected.name === name) {
          message += "an error with identical name but a different prototype.";
        } else {
          message += `"${name}"`;
        }
        const actualMessage = actual.message;
        if (actualMessage) {
          message += `

Error message:

${actualMessage}`;
        }
      } else {
        message += `"${lazyInspect()(actual, { depth: -1 })}"`;
      }
    }
    throwError = true;
  } else {
    const res = expected.apply({}, [actual]);
    if (res !== true) {
      if (!message) {
        generatedMessage = true;
        const name = expected.name ? `"${expected.name}" ` : "";
        const inspect = lazyInspect();
        message = `The ${name}validation function is expected to return "true". Received ${inspect(res)}`;
        if (Error.isError(actual)) {
          message += `

Caught error:

${actual}`;
        }
      }
      throwError = true;
    }
  }
  if (throwError) {
    if (AssertionError === undefined)
      loadAssertionError();
    const err = new AssertionError({
      actual,
      expected,
      message,
      operator: fn.name,
      stackStartFn: fn
    });
    err.generatedMessage = generatedMessage;
    throw err;
  }
}
function getActual(fn) {
  validateFunction(fn, "fn");
  try {
    fn();
  } catch (e) {
    return e;
  }
  return NO_EXCEPTION_SENTINEL;
}
function checkIsPromise(obj) {
  return isPromise(obj) || obj !== null && typeof obj === "object" && typeof obj.then === "function" && typeof obj.catch === "function";
}
async function waitForActual(promiseFn) {
  let resultPromise;
  if (typeof promiseFn === "function") {
    resultPromise = promiseFn();
    if (!checkIsPromise(resultPromise)) {
      throw codes.INVALID_RETURN_VALUE("instance of Promise", "promiseFn", resultPromise);
    }
  } else if (checkIsPromise(promiseFn)) {
    resultPromise = promiseFn;
  } else {
    throw codes.INVALID_ARG_TYPE("promiseFn", ["Function", "Promise"], promiseFn);
  }
  try {
    await resultPromise;
  } catch (e) {
    return e;
  }
  return NO_EXCEPTION_SENTINEL;
}
function expectsError(stackStartFn, actual, error, message) {
  if (typeof error === "string") {
    if (arguments.length === 4) {
      throw codes.INVALID_ARG_TYPE("error", ["Object", "Error", "Function", "RegExp"], error);
    }
    if (typeof actual === "object" && actual !== null) {
      if (actual.message === error) {
        throw codes.AMBIGUOUS_ARGUMENT("error/message", `The error message "${actual.message}" is identical to the message.`);
      }
      if (Object.keys(error).length === 0) {
        throw codes.INVALID_ARG_VALUE("error", error, "may not be an empty object");
      }
    } else if (actual === error) {
      throw codes.AMBIGUOUS_ARGUMENT("error/message", `The error "${actual}" is identical to the message.`);
    }
    message = error;
    error = undefined;
  } else if (error != null && typeof error !== "object" && typeof error !== "function") {
    throw codes.INVALID_ARG_TYPE("error", ["Object", "Error", "Function", "RegExp"], error);
  }
  if (actual === NO_EXCEPTION_SENTINEL) {
    let details = "";
    if (error?.name) {
      details += ` (${error.name})`;
    }
    details += message ? `: ${message}` : ".";
    const fnType = stackStartFn === kQualifiedStackNames["assert.rejects"] ? "rejection" : "exception";
    innerFail({
      actual: undefined,
      expected: error,
      operator: stackStartFn.name,
      message: `Missing expected ${fnType}${details}`,
      stackStartFn
    });
  }
  if (!error)
    return;
  expectedException(actual, error, message, stackStartFn);
}
function hasMatchingError(actual, expected) {
  if (typeof expected !== "function") {
    if (isRegExp(expected)) {
      const str = String(actual);
      return RegExpPrototypeExec.call(expected, str) !== null;
    }
    throw codes.INVALID_ARG_TYPE("expected", ["Function", "RegExp"], expected);
  }
  if (expected.prototype !== undefined && actual instanceof expected) {
    return true;
  }
  if (ObjectPrototypeIsPrototypeOf.call(Error, expected)) {
    return false;
  }
  return expected.apply({}, [actual]) === true;
}
function expectsNoError(stackStartFn, actual, error, message) {
  if (actual === NO_EXCEPTION_SENTINEL)
    return;
  if (typeof error === "string") {
    message = error;
    error = undefined;
  }
  if (!error || hasMatchingError(actual, error)) {
    const details = message ? `: ${message}` : ".";
    const fnType = stackStartFn === kQualifiedStackNames["assert.doesNotReject"] ? "rejection" : "exception";
    innerFail({
      actual,
      expected: error,
      operator: stackStartFn.name,
      message: `Got unwanted ${fnType}${details}
Actual message: "${actual?.message}"`,
      stackStartFn
    });
  }
  throw actual;
}
Assert.prototype.throws = function throws(promiseFn, ...args) {
  expectsError(throws, getActual(promiseFn), ...args);
};
const kQualifiedStackNames = {
  async "assert.rejects"(block, ...args) {
    expectsError(kQualifiedStackNames["assert.rejects"], await waitForActual(block), ...args);
  },
  async "assert.doesNotReject"(fn, ...args) {
    expectsNoError(kQualifiedStackNames["assert.doesNotReject"], await waitForActual(fn), ...args);
  }
};
Assert.prototype.rejects = kQualifiedStackNames["assert.rejects"];
Object.defineProperty(Assert.prototype.rejects, "name", { value: "rejects", configurable: true });
Assert.prototype.doesNotThrow = function doesNotThrow(fn, ...args) {
  expectsNoError(doesNotThrow, getActual(fn), ...args);
};
Assert.prototype.doesNotReject = kQualifiedStackNames["assert.doesNotReject"];
Object.defineProperty(Assert.prototype.doesNotReject, "name", { value: "doesNotReject", configurable: true });
Assert.prototype.ifError = function ifError(err) {
  if (err !== null && err !== undefined) {
    let message = "ifError got unwanted exception: ";
    const errMessage = typeof err === "object" ? err.message : undefined;
    if (typeof errMessage === "string") {
      let errConstructor;
      if (errMessage.length === 0 && (errConstructor = err.constructor)) {
        message += errConstructor.name;
      } else {
        message += errMessage;
      }
    } else {
      const inspect = lazyInspect();
      message += inspect(err);
    }
    if (AssertionError === undefined)
      loadAssertionError();
    const newErr = new AssertionError({
      actual: err,
      expected: null,
      operator: "ifError",
      message,
      stackStartFn: ifError,
      diff: this?.[kOptions]?.diff
    });
    const origStack = err.stack;
    if (typeof origStack === "string") {
      const origStackStart = StringPrototypeIndexOf.call(origStack, `
    at`);
      if (origStackStart !== -1) {
        const originalFrames = StringPrototypeSplit.call(StringPrototypeSlice.call(origStack, origStackStart + 1), `
`);
        let newFrames = StringPrototypeSplit.call(newErr.stack, `
`);
        for (const errFrame of originalFrames) {
          const pos = ArrayPrototypeIndexOf.call(newFrames, errFrame);
          if (pos !== -1) {
            newFrames = ArrayPrototypeSlice.call(newFrames, 0, pos);
            break;
          }
        }
        const stackStart = ArrayPrototypeJoin.call(newFrames, `
`);
        const stackEnd = ArrayPrototypeJoin.call(originalFrames, `
`);
        newErr.stack = `${stackStart}
${stackEnd}`;
      }
    }
    throw newErr;
  }
};
function internalMatch(string, regexp, message, fn) {
  if (!isRegExp(regexp)) {
    throw codes.INVALID_ARG_TYPE("regexp", ["RegExp"], regexp);
  }
  const match = fn === Assert.prototype.match;
  if (typeof string !== "string" || RegExpPrototypeExec.call(regexp, string) !== null !== match) {
    if (message instanceof Error) {
      throw message;
    }
    const generatedMessage = !message;
    const inspect = lazyInspect();
    message ||= typeof string !== "string" ? `The "string" argument must be of type string. Received type ${typeof string} (${inspect(string)})` : (match ? "The input did not match the regular expression " : "The input was expected to not match the regular expression ") + `${inspect(regexp)}. Input:

${inspect(string)}
`;
    if (AssertionError === undefined)
      loadAssertionError();
    const err = new AssertionError({
      actual: string,
      expected: regexp,
      message,
      operator: fn.name,
      stackStartFn: fn
    });
    err.generatedMessage = generatedMessage;
    throw err;
  }
}
Assert.prototype.match = function match(string, regexp, message) {
  internalMatch(string, regexp, message, match);
};
Assert.prototype.doesNotMatch = function doesNotMatch(string, regexp, message) {
  internalMatch(string, regexp, message, doesNotMatch);
};
var CallTracker;
Object.defineProperty(assert, "CallTracker", {
  get() {
    if (CallTracker === undefined) {
      const { deprecate } = host.utilInternal("deprecate");
      CallTracker = deprecate(callTrackerModule, "assert.CallTracker is deprecated.", "DEP0173");
    }
    return CallTracker;
  },
  set(value) {
    Object.defineProperty(this, "CallTracker", { value, writable: true, enumerable: true, configurable: true });
  },
  configurable: true,
  enumerable: true
});
function strict(...args) {
  innerOk(strict, args.length, ...args);
}
for (const name of [
  "ok",
  "fail",
  "equal",
  "notEqual",
  "deepEqual",
  "notDeepEqual",
  "deepStrictEqual",
  "notDeepStrictEqual",
  "strictEqual",
  "notStrictEqual",
  "partialDeepStrictEqual",
  "match",
  "doesNotMatch",
  "throws",
  "rejects",
  "doesNotThrow",
  "doesNotReject",
  "ifError"
]) {
  assert[name] = Assert.prototype[name];
}
for (const key of ObjectKeys(assert)) {
  ObjectDefineProperty(strict, key, ObjectGetOwnPropertyDescriptor(assert, key));
}
assert.strict = ObjectAssign(strict, {
  equal: assert.strictEqual,
  deepEqual: assert.deepStrictEqual,
  notEqual: assert.notStrictEqual,
  notDeepEqual: assert.notDeepStrictEqual
});
assert.strict.Assert = Assert;
assert.strict.strict = assert.strict;
assert.Assert = Assert;

return assert;
})
