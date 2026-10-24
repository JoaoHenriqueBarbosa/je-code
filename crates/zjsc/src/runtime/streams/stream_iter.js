// `ReadableStream.prototype.values` e `[Symbol.asyncIterator]` (o iterador assíncrono da especificação WHATWG, medido no
// bun 1.4.2). Programa escondido: recebe o `host` e devolve `{ values }`, chamado com o stream como `this`.
(function (host) {
  "use strict";

  const getReader = ReadableStream.prototype.getReader;
  // Função builtin não tem `prototype` próprio (como no JSC), então a cadeia sai de %AsyncGeneratorFunction.prototype%.
  const AsyncGeneratorFunctionPrototype = Object.getPrototypeOf(async function* () {});
  const AsyncIteratorPrototype = Object.getPrototypeOf(AsyncGeneratorFunctionPrototype.prototype);
  const states = new WeakMap();

  function stateOf(iterator, method) {
    const state = states.get(iterator);
    if (state === undefined) {
      host.throwNodeError(
        "TypeError",
        "ERR_INVALID_THIS",
        `Can only call ReadableStreamAsyncIterator.${method} on instances of ReadableStreamAsyncIterator`,
      );
    }
    return state;
  }

  // Cada chamada espera a anterior terminar (a fila de `next` e `return` da especificação).
  function enqueue(state, step) {
    state.ongoing = state.ongoing === null ? step() : state.ongoing.then(step, step);
    return state.ongoing;
  }

  const prototype = Object.create(AsyncIteratorPrototype);
  Object.defineProperties(prototype, {
    next: {
      value: function next() {
        let state;
        try {
          state = stateOf(this, "next");
        } catch (error) {
          return Promise.reject(error);
        }
        return enqueue(state, async () => {
          if (state.finished) return { value: undefined, done: true };
          let result;
          try {
            result = await state.reader.read();
          } catch (error) {
            state.finished = true;
            state.reader.releaseLock();
            throw error;
          }
          if (result.done) {
            state.finished = true;
            state.reader.releaseLock();
            return { value: undefined, done: true };
          }
          return { value: result.value, done: false };
        });
      },
      writable: true,
      configurable: true,
      enumerable: true,
    },
    return: {
      value: function (value) {
        let state;
        try {
          state = stateOf(this, "return");
        } catch (error) {
          return Promise.reject(error);
        }
        return enqueue(state, async () => {
          if (state.finished) return { value, done: true };
          state.finished = true;
          if (!state.preventCancel) {
            try {
              await state.reader.cancel(value);
            } finally {
              state.reader.releaseLock();
            }
            return { value, done: true };
          }
          state.reader.releaseLock();
          return { value, done: true };
        });
      },
      writable: true,
      configurable: true,
      enumerable: true,
    },
  });

  function values(options) {
    const reader = getReader.call(this);
    const preventCancel = options !== undefined && options !== null && Boolean(options.preventCancel);
    const iterator = Object.create(prototype);
    states.set(iterator, { reader, preventCancel, finished: false, ongoing: null });
    return iterator;
  }

  // `ReadableStream.from(iterável)`: um stream de contrapressão zero que puxa o iterador a cada `read`.
  function from(iterable) {
    if (iterable === undefined || iterable === null) {
      // O texto é o do JSC para o argumento, não o fonte do chamador: sem argumento vazio, `undefined` vira `void 0`.
      const shown = arguments.length === 0 ? "" : iterable === null ? "null" : "void 0";
      throw new TypeError(`${String(iterable)} is not an object (evaluating 'ReadableStream.from(${shown})')`);
    }
    let method = iterable[Symbol.asyncIterator];
    const isAsync = method !== undefined && method !== null;
    if (!isAsync) method = iterable[Symbol.iterator];
    if (typeof method !== "function") {
      host.throwNodeError("TypeError", "ERR_ARG_NOT_ITERABLE", `${host.inspect(iterable)} must be iterable`);
    }
    const iterator = method.call(iterable);
    const next = iterator.next;
    return new ReadableStream(
      {
        async pull(controller) {
          let result = next.call(iterator);
          if (isAsync) result = await result;
          if (result === null || typeof result !== "object") {
            throw new TypeError("iterator.next() returned a non-object value");
          }
          if (result.done) {
            controller.close();
            return;
          }
          controller.enqueue(isAsync ? result.value : await result.value);
        },
        async cancel(reason) {
          const close = iterator.return;
          if (close !== undefined && close !== null) await close.call(iterator, reason);
        },
      },
      { highWaterMark: 0 },
    );
  }

  return { values, from };
});
