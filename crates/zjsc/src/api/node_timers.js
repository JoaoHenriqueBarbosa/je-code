// `node:timers` do bun 1.4.2: o `src/js/node/timers.ts`, com `$isCallable` como `typeof === "function"` e o
// `require("node:timers/promises")` pelo `host`.
(function (host) {
  "use strict";

  const { throwNotImplemented } = host.internal;

  var timersPromisesValue;

  return {
    setTimeout,
    clearTimeout,
    setInterval,
    setImmediate,
    clearInterval,
    clearImmediate,
    get promises() {
      return (timersPromisesValue ??= host.requireBuiltin("node:timers/promises"));
    },
    set promises(value) {
      timersPromisesValue = value;
    },
    active(timer) {
      if (typeof timer?.refresh === "function") {
        timer.refresh();
      } else {
        throwNotImplemented("'timers.active'");
      }
    },
    unenroll(timer) {
      if (typeof timer?.refresh === "function") {
        clearTimeout(timer);
        return;
      }

      throwNotImplemented("'timers.unenroll'");
    },
    enroll(timer, _msecs) {
      if (typeof timer?.refresh === "function") {
        timer.refresh();
        return;
      }

      throwNotImplemented("'timers.enroll'");
    },
  };
})
