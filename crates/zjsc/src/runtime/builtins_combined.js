(function (executor)
{
    "use strict";

    if (!@isCallable(executor))
        @throwTypeError("Promise constructor takes a function argument");

    var promise = @createPromise(this);
    var capturedPromise = promise;

    try {
        executor(
            (resolution) => {
                return @resolvePromiseWithFirstResolvingFunctionCallCheck(capturedPromise, resolution);
            },
            (reason) => {
                return @rejectPromiseWithFirstResolvingFunctionCallCheck(capturedPromise, reason);
            });
    } catch (error) {
        @rejectPromiseWithFirstResolvingFunctionCallCheck(promise, error);
    }

    return promise;
})
(function (disposeCapability, value, isAsync )
{
    'use strict';

    @assert(@isArray(disposeCapability));

    var resource;
    if (@argumentCount() < 4) {
        if (@isUndefinedOrNull(value) && !isAsync)
            return;
        resource = @createDisposableResource(value, isAsync);
    } else {
        @assert(value === @undefined);
        resource = @createDisposableResource(@undefined, isAsync, @argument(3));
    }

    @arrayPush(disposeCapability, resource);
})
(function (value, onAsyncDispose)
{
    'use strict';

    if (!@isAsyncDisposableStack(this))
        @throwTypeError("AsyncDisposableStack.prototype.adopt requires that |this| be a AsyncDisposableStack object");

    if (@getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldState) === @AsyncDisposableStackStateDisposed)
        throw new @ReferenceError("AsyncDisposableStack.prototype.adopt requires that |this| be a pending AsyncDisposableStack object");

    if (!@isCallable(onAsyncDispose))
        @throwTypeError("AsyncDisposableStack.prototype.adopt requires that onAsyncDispose argument be a callable");

    var closure = () => onAsyncDispose.@call(@undefined, value);
    @addDisposableResource(@getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldCapability), @undefined,  true, closure);

    return value;
})
(function (value, onDispose)
{
    'use strict';

    if (!@isDisposableStack(this))
        @throwTypeError("DisposableStack.prototype.adopt requires that |this| be a DisposableStack object");

    if (@getDisposableStackInternalField(this, @disposableStackFieldState) === @DisposableStackStateDisposed)
        throw new @ReferenceError("DisposableStack.prototype.adopt requires that |this| be a pending DisposableStack object");

    if (!@isCallable(onDispose))
        @throwTypeError("DisposableStack.prototype.adopt requires that onDispose argument be a callable");

    var closure = () => { onDispose.@call(@undefined, value); }
    @addDisposableResource(@getDisposableStackInternalField(this, @disposableStackFieldCapability), @undefined,  false, closure);

    return value;
})
(function (thisValue, argumentValues)
{
    "use strict";

    return this.@apply(thisValue, argumentValues);
})
(function (target, thisArgument, argumentsList)
{
    "use strict";

    if (!@isCallable(target))
        @throwTypeError("Reflect.apply requires the first argument be a function");

    if (!@isObject(argumentsList))
        @throwTypeError("Reflect.apply requires the third argument be an object");

    return target.@apply(thisArgument, argumentsList);
})
(function (array, kind, index)
{
    "use strict";
    var done = true;
    var value;

    var length = @isTypedArrayView(array) ? @typedArrayLength(array) : @toLength(array.length);
    if (index < length) {
        @putArrayIteratorInternalField(this, @arrayIteratorFieldIndex, index + 1);
        done = false;
        if (kind === @iterationKindKey)
            value = index;
        else {
            value = array[index];
            if (kind === @iterationKindEntries)
                value = [index, value];
        }
    } else
        @putArrayIteratorInternalField(this, @arrayIteratorFieldIndex, -1);

    return { value, done };
})
(function ()
{
    'use strict';

    try {
        var returnMethod = this.return;
        if (@isUndefinedOrNull(returnMethod))
            return @newResolvedPromise(@undefined);

        var result = returnMethod.@call(this);
        var resultWrapper;
        if (@isPromise(result) && result.constructor === @Promise)
            resultWrapper = result;
        else
            resultWrapper = @newResolvedPromise(result);
        var promise = @newPromise();
        @performPromiseThen(resultWrapper, @promiseReturnUndefinedOnFulfilled, @undefined, promise);
        return promise;
    } catch (e) {
        return @newRejectedPromise(e);
    }
})
(function (index)
{
    "use strict";

    var array = @toObject(this, "Array.prototype.at requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    var k = @toIntegerOrInfinity(index);
    if (k < 0)
        k += length;

    return (k >= 0 && k < length) ? array[k] : @undefined;
})
(function (index)
{
    "use strict";

    var length = @typedArrayLength(this);

    var k = @toIntegerOrInfinity(index);
    if (k < 0)
        k += length;

    return (k >= 0 && k < length) ? this[k] : @undefined;
})
(function (map)
{
    "use strict";
    
    if (!@isMap(map))
        @throwTypeError("builtinMapIterable called with non-Map object.");

    //
    //
    //
    var iteratorFunction = map.@entries;

    return @wrappedIterator(iteratorFunction.@call(map));
})
(function (set)
{
    "use strict";
    
    if (!@isSet(set))
        @throwTypeError("builtinSetIterable called with non-Set object.");

    //
    //
    //
    var iteratorFunction = set.@values;
    
    return @wrappedIterator(iteratorFunction.@call(set));
})
(function (thisArgument)
{
    "use strict";

    var argumentValues = [];
    //
    for (var i = 1; i < @argumentCount(); i++)
        @putByValDirect(argumentValues, i-1, arguments[i]);

    return this.@apply(thisArgument, argumentValues);
})
(function (chunkSize)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.chunks requires that |this| be an Object.");

    if (typeof chunkSize !== "number" || chunkSize % 1 !== 0) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.chunks requires that first argument be an integral Number.");
        }
    }

    if (chunkSize < 1 || chunkSize > 4294967295) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwRangeError("Iterator.prototype.chunks requires that first argument be between 1 and 2**32 - 1.");
        }
    }

    var iterated = this;
    var iteratedNextMethod = this.next;

    var generator = (function*() {
        var buffer = [];

        for (;;) {
            var result = @iteratorGenericNext(iteratedNextMethod, iterated);
            if (result.done) {
                if (buffer.length)
                    yield buffer;
                return;
            }

            @arrayPush(buffer, result.value);

            if (buffer.length === chunkSize) {
                @ifAbruptCloseIterator(iterated, (
                    yield buffer
                ));

                buffer = [];
            }
        }
    })();

    return @iteratorHelperCreate(generator, iterated);
})
(function (openIters)
{
    "use strict";

    for (var i = openIters.length - 1; i >= 0; --i) {
        try {
            @iteratorGenericClose(openIters[i]);
        } catch (e) { }
    }
})
(function ()
{
    "use strict";

    var argumentCount = @argumentCount();

    var openMethods = [];
    var iterables = [];
    for (var i = 0; i < argumentCount; ++i) {
        var iterable = arguments[i];
        if (!@isObject(iterable))
            @throwTypeError("Iterator.concat expects all arguments to be objects");

        var openMethod = iterable.@@iterator;
        if (!@isCallable(openMethod))
            @throwTypeError("Iterator.concat expects all arguments to be iterable");

        @arrayPush(openMethods, openMethod);
        @arrayPush(iterables, iterable);
    }

    var generator = (function*() {
        for (var i = 0; i < argumentCount; ++i) {
            var iterator = openMethods[i].@call(iterables[i]);
            if (!@isObject(iterator))
                @throwTypeError("Iterator.concat expects all arguments to be iterable");

            var nextMethod = iterator.next;

            for (;;) {
                var result = @iteratorGenericNext(nextMethod, iterator);
                if (result.done)
                    break;

                var value = result.value;
                @ifAbruptCloseIterator(iterator, (
                    yield value
                ));
            }
        }
    })();

    return @iteratorHelperCreate(generator, null);
})
(function ()
{
    let array = new @Array(arguments.length);
    array.__proto__ = null;
    
    for (let i = 0; i < arguments.length; ++i)
        @arrayPush(array, arguments[i]);
    
    return array;
})
(function (value, isAsync )
{
    'use strict';

    var method;
    if (@argumentCount() < 3) {
        if (@isUndefinedOrNull(value))
            value = @undefined;
        else {
            if (!@isObject(value))
                @throwTypeError("Disposable value must be an object");
            if (isAsync)
                method = @getAsyncDisposeMethod(value);
            else
                method = @getDisposeMethod(value);
        }
    } else {
        method = @argument(2);
        if (!@isCallable(method))
            @throwTypeError("Callback that is called on dispose must be callable");
    }

    return { value, method };
})
(function (InjectedScriptHost, inspectedGlobalObject, injectedScriptId)
{

function PrototypelessObjectBase() {}
PrototypelessObjectBase.prototype = null;

function toString(obj)
{
    return @String(obj);
}

function toStringDescription(obj)
{
    if (obj === 0 && 1 / obj < 0)
        return "-0";

    if (isBigInt(obj))
        return toString(obj) + "n";

    return toString(obj);
}

function isUInt32(obj)
{
    if (typeof obj === "number")
        return obj >>> 0 === obj && (obj > 0 || 1 / obj > 0);
    return "" + (obj >>> 0) === obj;
}

function isSymbol(value)
{
    return typeof value === "symbol";
}

function isBigInt(value)
{
    return typeof value === "bigint";
}

function isEmptyObject(object)
{
    for (let key in object)
        return false;
    return true;
}

function isDefined(value)
{
    return !!value || InjectedScriptHost.isHTMLAllCollection(value);
}

function isPrimitiveValue(value)
{
    switch (typeof value) {
    case "boolean":
    case "number":
    case "string":
        return true;
    case "undefined":
        return !InjectedScriptHost.isHTMLAllCollection(value);
    default:
        return false;
    }
}
    
function createIterableWithoutPrototypeFromArguments(argumentsObject) {
    let iteratorFunction = argumentsObject.@@iterator;
    return @wrappedIterator(iteratorFunction.@call(argumentsObject));
}

function max(a, b) {
    return a > b ? a : b;
}

//

let InjectedScript = class InjectedScript extends PrototypelessObjectBase
{
    constructor()
    {
        super();

        this._lastBoundObjectId = 1;
        this._idToWrappedObject = @createObjectWithoutPrototype();
        this._idToObjectGroupName = @createObjectWithoutPrototype();
        this._objectGroups = @createObjectWithoutPrototype();
        this._modules = @createObjectWithoutPrototype();
        this._nextSavedResultIndex = 1;
        this._savedResults = @createArrayWithoutPrototype();
    }

    //

    execute(functionString, objectGroup, includeCommandLineAPI, returnByValue, generatePreview, saveResult, args)
    {
        return this._wrapAndSaveCall(objectGroup, returnByValue, generatePreview, saveResult, () => {
            const isEvalOnCallFrame = false;
            return this._evaluateOn(InjectedScriptHost.evaluateWithScopeExtension, InjectedScriptHost, functionString, isEvalOnCallFrame, includeCommandLineAPI).@apply(@undefined, args);
        });
    }

    evaluate(expression, objectGroup, includeCommandLineAPI, returnByValue, generatePreview, saveResult)
    {
        const isEvalOnCallFrame = false;
        return this._evaluateAndWrap(InjectedScriptHost.evaluateWithScopeExtension, InjectedScriptHost, expression, objectGroup, isEvalOnCallFrame, includeCommandLineAPI, returnByValue, generatePreview, saveResult);
    }

    awaitPromise(promiseObjectId, returnByValue, generatePreview, saveResult, callback)
    {
        let parsedPromiseObjectId = this._parseObjectId(promiseObjectId);
        let promiseObject = this._objectForId(parsedPromiseObjectId);
        let promiseObjectGroupName = this._idToObjectGroupName[parsedPromiseObjectId.id];

        if (!isDefined(promiseObject)) {
            callback("Could not find object with given id");
            return;
        }

        if (!@isPromise(promiseObject)) {
            callback("Object with given id is not a Promise");
            return;
        }

        let resolve = (value) => {
            let returnObject = @createObjectWithoutPrototype(
                "wasThrown", false,
                "result", RemoteObject.create(value, promiseObjectGroupName, returnByValue, generatePreview),
            );

            if (saveResult) {
                this._savedResultIndex = 0;
                this._saveResult(returnObject.result);
                if (this._savedResultIndex)
                    returnObject.savedResultIndex = this._savedResultIndex;
            }

            callback(returnObject);
        };
        let reject = (reason) => {
            callback(this._createThrownValue(reason, promiseObjectGroupName));
        };
        promiseObject.@then(resolve, reject);
    }

    evaluateOnCallFrame(topCallFrame, callFrameId, expression, objectGroup, includeCommandLineAPI, returnByValue, generatePreview, saveResult)
    {
        let callFrame = this._callFrameForId(topCallFrame, callFrameId);
        if (!callFrame)
            return "Could not find call frame with given id";
        const isEvalOnCallFrame = true;
        return this._evaluateAndWrap(callFrame.evaluateWithScopeExtension, callFrame, expression, objectGroup, isEvalOnCallFrame, includeCommandLineAPI, returnByValue, generatePreview, saveResult);
    }

    callFunctionOn(objectId, expression, args, returnByValue, generatePreview, awaitPromise, callback)
    {
        let parsedObjectId = this._parseObjectId(objectId);
        let object = this._objectForId(parsedObjectId);
        let objectGroupName = this._idToObjectGroupName[parsedObjectId.id];

        if (!isDefined(object)) {
            callback("Could not find object with given id");
            return;
        }

        let resolvedArgs = @createArrayWithoutPrototype();
        if (args) {
            let callArgs = InjectedScriptHost.evaluate(args);
            for (let i = 0; i < callArgs.length; ++i) {
                try {
                    resolvedArgs[i] = this._resolveCallArgument(callArgs[i]);
                } catch (e) {
                    callback(@String(e));
                    return;
                }
            }
        }

        try {
            let func = InjectedScriptHost.evaluate("(" + expression + ")");
            if (typeof func !== "function") {
                callback("Given expression does not evaluate to a function");
                return;
            }
            let result = func.@apply(object, resolvedArgs);
            if (awaitPromise && isDefined(result) && @isPromise(result)) {
                result.then((value) => {
                    callback(@createObjectWithoutPrototype(
                        "wasThrown", false,
                        "result", RemoteObject.create(value, objectGroupName, returnByValue, generatePreview),
                    ));
                }, (reason) => {
                    callback(this._createThrownValue(reason, objectGroupName));
                });
            } else {
                callback(@createObjectWithoutPrototype(
                    "wasThrown", false,
                    "result", RemoteObject.create(result, objectGroupName, returnByValue, generatePreview),
                ));
            }
        } catch (e) {
            callback(this._createThrownValue(e, objectGroupName));
            return;
        }
    }

    getFunctionDetails(objectId)
    {
        let parsedObjectId = this._parseObjectId(objectId);
        let object = this._objectForId(parsedObjectId);
        if (typeof object !== "function")
            return "Cannot resolve function by id.";
        return this.functionDetails(object);
    }

    functionDetails(func)
    {
        let details = InjectedScriptHost.functionDetails(func);
        if (!details)
            return "Cannot resolve function details.";
        return details;
    }

    getPreview(objectId)
    {
        let parsedObjectId = this._parseObjectId(objectId);
        let object = this._objectForId(parsedObjectId);
        return RemoteObject.createObjectPreviewForValue(object, true);
    }

    getProperties(objectId, ownProperties, fetchStart, fetchCount, generatePreview)
    {
        let collectionMode = ownProperties ? InjectedScript.CollectionMode.OwnProperties : InjectedScript.CollectionMode.AllProperties;
        return this._getProperties(objectId, collectionMode, @createObjectWithoutPrototype(
            "fetchStart", fetchStart,
            "fetchCount", fetchCount,
            "generatePreview", generatePreview,
        ));
    }

    getDisplayableProperties(objectId, fetchStart, fetchCount, generatePreview)
    {
        let collectionMode = InjectedScript.CollectionMode.OwnProperties | InjectedScript.CollectionMode.NativeGetterProperties;
        return this._getProperties(objectId, collectionMode, @createObjectWithoutPrototype(
            "fetchStart", fetchStart,
            "fetchCount", fetchCount,
            "generatePreview", generatePreview,
            "nativeGettersAsValues", true,
        ));
    }

    getInternalProperties(objectId, generatePreview)
    {
        let parsedObjectId = this._parseObjectId(objectId);
        let object = this._objectForId(parsedObjectId);
        let objectGroupName = this._idToObjectGroupName[parsedObjectId.id];

        if (!isDefined(object))
            return false;

        if (isSymbol(object))
            return false;

        let descriptors = this._internalPropertyDescriptors(object);
        if (!descriptors)
            return @createArrayWithoutPrototype();

        for (let i = 0; i < descriptors.length; ++i) {
            let descriptor = descriptors[i];
            if ("value" in descriptor)
                descriptor.value = RemoteObject.create(descriptor.value, objectGroupName, false, generatePreview);
        }

        return descriptors;
    }

    getCollectionEntries(objectId, objectGroupName, fetchStart, fetchCount)
    {
        let parsedObjectId = this._parseObjectId(objectId);
        let object = this._objectForId(parsedObjectId);
        objectGroupName = objectGroupName || this._idToObjectGroupName[parsedObjectId.id];

        if (!isDefined(object))
            return;

        if (typeof object !== "object")
            return;

        let entries = this._entries(object, InjectedScriptHost.subtype(object), fetchStart, fetchCount);
        for (let i = 0; i < entries.length; ++i) {
            let entry = entries[i];
            entry.value = RemoteObject.create(entry.value, objectGroupName, false, true);
            if ("key" in entry)
                entry.key = RemoteObject.create(entry.key, objectGroupName, false, true);
        }
        return entries;
    }

    saveResult(callArgumentJSON)
    {
        this._savedResultIndex = 0;

        try {
            let callArgument = InjectedScriptHost.evaluate("(" + callArgumentJSON + ")");
            let value = this._resolveCallArgument(callArgument);
            this._saveResult(value);
        } catch { }

        return this._savedResultIndex;
    }

    wrapCallFrames(callFrame)
    {
        if (!callFrame)
            return false;

        let result = @createArrayWithoutPrototype();
        let depth = 0;
        do {
            @arrayPush(result, new InjectedScript.CallFrameProxy(depth++, callFrame));
            callFrame = callFrame.caller;
        } while (callFrame);
        return result;
    }

    wrapObject(object, groupName, canAccessInspectedGlobalObject, generatePreview)
    {
        if (!canAccessInspectedGlobalObject)
            return this._fallbackWrapper(object);

        return RemoteObject.create(object, groupName, false, generatePreview);
    }

    wrapJSONString(jsonString, groupName, generatePreview)
    {
        try {
            return this.wrapObject(@jsonParse(jsonString), groupName, true, generatePreview);
        } catch {
            return null;
        }
    }

    wrapTable(canAccessInspectedGlobalObject, table, columns)
    {
        if (!canAccessInspectedGlobalObject)
            return this._fallbackWrapper(table);

        //
        //
        //

        let columnNames = null;
        if (typeof columns === "string")
            columns = @createArrayWithoutPrototype(columns);

        if (InjectedScriptHost.subtype(columns) === "array") {
            columnNames = @createArrayWithoutPrototype();
            for (let i = 0; i < columns.length; ++i)
                @arrayPush(columnNames, toString(columns[i]));
        }

        return RemoteObject.create(table, "console", false, true, columnNames);
    }

    previewValue(value)
    {
        return RemoteObject.createObjectPreviewForValue(value, true);
    }

    setEventValue(value)
    {
        this._eventValue = value;
    }

    clearEventValue()
    {
        delete this._eventValue;
    }

    setExceptionValue(value)
    {
        this._exceptionValue = value;
    }

    clearExceptionValue()
    {
        delete this._exceptionValue;
    }

    findObjectById(objectId)
    {
        let parsedObjectId = this._parseObjectId(objectId);
        return this._objectForId(parsedObjectId);
    }

    releaseObject(objectId)
    {
        let parsedObjectId = this._parseObjectId(objectId);
        this._releaseObject(parsedObjectId.id);
    }

    releaseObjectGroup(objectGroupName)
    {
        if (objectGroupName === "console") {
            delete this._lastResult;
            this._nextSavedResultIndex = 1;
            this._savedResults = @createArrayWithoutPrototype();
        }

        let group = this._objectGroups[objectGroupName];
        if (!group)
            return;

        for (let i = 0; i < group.length; i++)
            this._releaseObject(group[i]);

        delete this._objectGroups[objectGroupName];
    }

    createCommandLineAPIObject(callFrame)
    {
        return new CommandLineAPI(callFrame || null);
    }

    //

    inspectObject(object)
    {
        if (this._inspectObject)
            this._inspectObject(object);
    }

    //

    hasInjectedModule(name)
    {
        return this._modules[name];
    }

    injectModule(name, moduleFunction, host)
    {
        this._modules[name] = false;

        if (typeof moduleFunction !== "function")
            throw "Error: Web Inspector: a function was expected for injectModule";
        moduleFunction(InjectedScriptHost, inspectedGlobalObject, injectedScriptId, this, @createObjectWithoutPrototype(
            "RemoteObject", RemoteObject,
            "CommandLineAPI", CommandLineAPI,
        ), host);

        this._modules[name] = true;
    }

    //

    isPrimitiveValue(value)
    {
        return isPrimitiveValue(value);
    }

    //

    _parseObjectId(objectId)
    {
        return InjectedScriptHost.evaluate("(" + objectId + ")");
    }

    _objectForId(objectId)
    {
        return this._idToWrappedObject[objectId.id];
    }

    _bind(object, objectGroupName)
    {
        let id = this._lastBoundObjectId++;
        let objectId = `{"injectedScriptId":${injectedScriptId},"id":${id}}`;

        this._idToWrappedObject[id] = object;

        if (objectGroupName) {
            let group = this._objectGroups[objectGroupName];
            if (!group) {
                group = @createArrayWithoutPrototype();
                this._objectGroups[objectGroupName] = group;
            }
            @arrayPush(group, id);
            this._idToObjectGroupName[id] = objectGroupName;
        }

        return objectId;
    }

    _releaseObject(id)
    {
        delete this._idToWrappedObject[id];
        delete this._idToObjectGroupName[id];
    }

    _fallbackWrapper(object)
    {
        let result = @createObjectWithoutPrototype("type", typeof object);
        if (isPrimitiveValue(object))
            result.value = object;
        else
            result.description = toStringDescription(object);
        return result;
    }

    _resolveCallArgument(callArgumentJSON)
    {
        if ("value" in callArgumentJSON)
            return callArgumentJSON.value;

        let objectId = callArgumentJSON.objectId;
        if (objectId) {
            let parsedArgId = this._parseObjectId(objectId);
            if (!parsedArgId || parsedArgId["injectedScriptId"] !== injectedScriptId)
                throw "Arguments should belong to the same JavaScript world as the target object.";

            let resolvedArg = this._objectForId(parsedArgId);
            if (!isDefined(resolvedArg))
                throw "Could not find object with given id";

            return resolvedArg;
        }

        return @undefined;
    }

    _createThrownValue(value, objectGroup)
    {
        let remoteObject = RemoteObject.create(value, objectGroup);
        try {
            remoteObject.description = toStringDescription(value);
        } catch { }
        return @createObjectWithoutPrototype(
            "wasThrown", true,
            "result", remoteObject,
        );
    }

    _evaluateAndWrap(evalFunction, object, expression, objectGroup, isEvalOnCallFrame, includeCommandLineAPI, returnByValue, generatePreview, saveResult)
    {
        return this._wrapAndSaveCall(objectGroup, returnByValue, generatePreview, saveResult, () => {
            return this._evaluateOn(evalFunction, object, expression, isEvalOnCallFrame, includeCommandLineAPI);
        });
    }

    _wrapAndSaveCall(objectGroup, returnByValue, generatePreview, saveResult, func)
    {
        return this._wrapCall(objectGroup, returnByValue, generatePreview, saveResult, () => {
            let result = func();
            if (saveResult)
                this._saveResult(result);
            return result;
        });
    }

    _wrapCall(objectGroup, returnByValue, generatePreview, saveResult, func)
    {
        try {
            this._savedResultIndex = 0;

            let returnObject = @createObjectWithoutPrototype(
                "wasThrown", false,
                "result", RemoteObject.create(func(), objectGroup, returnByValue, generatePreview),
            );

            if (saveResult && this._savedResultIndex)
                returnObject.savedResultIndex = this._savedResultIndex;

            return returnObject;
        } catch (e) {
            return this._createThrownValue(e, objectGroup);
        }
    }

    _evaluateOn(evalFunction, object, expression, isEvalOnCallFrame, includeCommandLineAPI)
    {
        let commandLineAPI = null;
        if (includeCommandLineAPI)
            commandLineAPI = this.createCommandLineAPIObject(isEvalOnCallFrame ? object : null);
        return evalFunction.@call(object, expression, commandLineAPI);
    }

    _callFrameForId(topCallFrame, callFrameId)
    {
        let parsedCallFrameId = InjectedScriptHost.evaluate("(" + callFrameId + ")");
        let ordinal = parsedCallFrameId["ordinal"];
        let callFrame = topCallFrame;
        while (--ordinal >= 0 && callFrame)
            callFrame = callFrame.caller;
        return callFrame;
    }

    _getProperties(objectId, collectionMode, {fetchStart, fetchCount, generatePreview, nativeGettersAsValues})
    {
        let parsedObjectId = this._parseObjectId(objectId);
        let object = this._objectForId(parsedObjectId);
        let objectGroupName = this._idToObjectGroupName[parsedObjectId.id];

        if (!isDefined(object))
            return false;

        if (isSymbol(object))
            return false;

        let start = fetchStart || 0;
        if (start < 0)
            start = 0;

        let count = fetchCount || 0;
        if (count < 0)
            count = 0;

        //
        let includeProto = !start;

        let descriptors = @createArrayWithoutPrototype();
        this._forEachPropertyDescriptor(object, collectionMode, (descriptor) => {
            if (start > 0) {
                --start;
                return InjectedScript.PropertyFetchAction.Continue;
            }

            if ("get" in descriptor)
                descriptor.get = RemoteObject.create(descriptor.get, objectGroupName);
            if ("set" in descriptor)
                descriptor.set = RemoteObject.create(descriptor.set, objectGroupName);
            if ("value" in descriptor)
                descriptor.value = RemoteObject.create(descriptor.value, objectGroupName, false, generatePreview);
            if ("symbol" in descriptor)
                descriptor.symbol = RemoteObject.create(descriptor.symbol, objectGroupName);
            @arrayPush(descriptors, descriptor);

            if (includeProto && count && descriptors.length >= count && descriptor.name !== "__proto__")
                return InjectedScript.PropertyFetchAction.Stop;

            return (count && descriptors.length >= count) ? InjectedScript.PropertyFetchAction.Stop : InjectedScript.PropertyFetchAction.Continue;
        }, @createObjectWithoutPrototype(
            "nativeGettersAsValues", nativeGettersAsValues,
            "includeProto", includeProto,
        ));
        return descriptors;
    }

    _internalPropertyDescriptors(object, completeDescriptor)
    {
        let internalProperties = InjectedScriptHost.getInternalProperties(object);
        if (!internalProperties)
            return null;

        let descriptors = @createArrayWithoutPrototype();
        for (let i = 0; i < internalProperties.length; i++) {
            let property = internalProperties[i];
            let descriptor = @createObjectWithoutPrototype(
                "name", property.name,
                "value", property.value,
            );
            if (completeDescriptor)
                descriptor.isOwn = true;
            @arrayPush(descriptors, descriptor);
        }
        return descriptors;
    }

    _forEachPropertyDescriptor(object, collectionMode, callback, {nativeGettersAsValues, includeProto})
    {
        let subtype = RemoteObject.subtype(object);
        if (subtype === "proxy" || subtype === "weakref")
            return;

        let nameProcessed = new @Set;

        //
        nameProcessed.@add("__proto__");

        function createFakeValueDescriptor(name, symbol, descriptor, isOwnProperty, possibleNativeBindingGetter)
        {
            try {
                let fakeDescriptor = @createObjectWithoutPrototype(
                    "name", name,
                    "value", object[name],
                );
                if (descriptor) {
                    if (descriptor.writable)
                        fakeDescriptor.writable = true;
                    if (descriptor.configurable)
                        fakeDescriptor.configurable = true;
                    if (descriptor.enumerable)
                        fakeDescriptor.enumerable = true;
                }
                if (possibleNativeBindingGetter)
                    fakeDescriptor.nativeGetter = true;
                if (isOwnProperty)
                    fakeDescriptor.isOwn = true;
                if (symbol)
                    fakeDescriptor.symbol = symbol;
                //
                if (@isPromise(fakeDescriptor.value) && InjectedScriptHost.isPromiseRejectedWithNativeGetterTypeError(fakeDescriptor.value))
                    fakeDescriptor.value.@then(@undefined, function(){});
                return fakeDescriptor;
            } catch (e) {
                let errorDescriptor = @createObjectWithoutPrototype(
                    "name", name,
                    "value", e,
                    "wasThrown", true,
                );
                if (isOwnProperty)
                    errorDescriptor.isOwn = true;
                if (symbol)
                    errorDescriptor.symbol = symbol;
                return errorDescriptor;
            }
        }

        function processDescriptor(descriptor, isOwnProperty, possibleNativeBindingGetter)
        {
            //
            if (collectionMode & InjectedScript.CollectionMode.AllProperties)
                return callback(descriptor);

            //
            if (collectionMode & InjectedScript.CollectionMode.OwnProperties && isOwnProperty)
                return callback(descriptor);

            //
            if (collectionMode & InjectedScript.CollectionMode.NativeGetterProperties) {
                if (possibleNativeBindingGetter)
                    return callback(descriptor);
            }
        }

        function processProperty(o, propertyName, isOwnProperty, isPrivate)
        {
            if (nameProcessed.@has(propertyName))
                return InjectedScript.PropertyFetchAction.Continue;

            nameProcessed.@add(propertyName);

            //
            let name = isPrivate ? propertyName.description : toString(propertyName);
            let symbol = (!isPrivate && isSymbol(propertyName)) ? propertyName : null;

            let descriptor = @Object.@getOwnPropertyDescriptor(o, propertyName);
            if (!descriptor) {
                //
                //
                let fakeDescriptor = createFakeValueDescriptor(name, symbol, descriptor, isOwnProperty);
                return processDescriptor(fakeDescriptor, isOwnProperty);
            }

            if (nativeGettersAsValues) {
                if (@String(descriptor.get).@endsWith("[native code] }") || (!descriptor.get && @Object.@hasOwn(descriptor, "get") && !descriptor.set && @Object.@hasOwn(descriptor, "set"))) {
                    //
                    //
                    let fakeDescriptor = createFakeValueDescriptor(name, symbol, descriptor, isOwnProperty, true);
                    return processDescriptor(fakeDescriptor, isOwnProperty, true);
                }
            }

            descriptor.name = name;
            if (isOwnProperty)
                descriptor.isOwn = true;
            if (symbol)
                descriptor.symbol = symbol;
            if (isPrivate)
                descriptor.isPrivate = true;
            return processDescriptor(descriptor, isOwnProperty);
        }

        let isArrayLike = false;
        try {
            isArrayLike = subtype === "array" && @isFinite(object.length) && object.length > 0;
        } catch { }

        for (let o = object; isDefined(o); o = @Object.@getPrototypeOf(o)) {
            let isOwnProperty = o === object;
            let shouldBreak = false;

            let privatePropertySymbols = InjectedScriptHost.getOwnPrivatePropertySymbols(o);
            for (let i = 0; i < privatePropertySymbols.length; ++i) {
                let privatePropertySymbol = privatePropertySymbols[i];
                let result = processProperty(o, privatePropertySymbol, isOwnProperty, true);
                shouldBreak = result === InjectedScript.PropertyFetchAction.Stop;
                if (shouldBreak)
                    break;
            }

            if (shouldBreak)
                break;

            let privateMethods = InjectedScriptHost.getOwnPrivatePropertyMethods(o, isOwnProperty);
            for (let i = 0; i < privateMethods.length; ++i) {
                let privateMethod = privateMethods[i];
                let descriptor = @createObjectWithoutPrototype();
                descriptor.name = privateMethod.name;
                if (@Object.@hasOwn(privateMethod, "value"))
                    descriptor.value = privateMethod.value;
                if (@Object.@hasOwn(privateMethod, "get"))
                    descriptor.get = privateMethod.get;
                if (@Object.@hasOwn(privateMethod, "set"))
                    descriptor.set = privateMethod.set;
                if (isOwnProperty)
                    descriptor.isOwn = true;
                descriptor.isPrivate = true;
                let result = processDescriptor(descriptor, isOwnProperty);
                shouldBreak = result === InjectedScript.PropertyFetchAction.Stop;
                if (shouldBreak)
                    break;
            }

            if (shouldBreak)
                break;

            //
            if (isArrayLike && isOwnProperty) {
                for (let i = 0; i < o.length; ++i) {
                    if (!(i in o))
                        continue;

                    let result = processProperty(o, toString(i), isOwnProperty);
                    shouldBreak = result === InjectedScript.PropertyFetchAction.Stop;
                    if (shouldBreak)
                        break;
                }
            } else {
                let propertyNames = @Object.@getOwnPropertyNames(o);
                for (let i = 0; i < propertyNames.length; ++i) {
                    let result = processProperty(o, propertyNames[i], isOwnProperty);
                    shouldBreak = result === InjectedScript.PropertyFetchAction.Stop;
                    if (shouldBreak)
                        break;
                }
            }

            if (shouldBreak)
                break;

            let propertySymbols = @Object.@getOwnPropertySymbols(o);
            for (let i = 0; i < propertySymbols.length; ++i) {
                let result = processProperty(o, propertySymbols[i], isOwnProperty);
                shouldBreak = result === InjectedScript.PropertyFetchAction.Stop;
                if (shouldBreak)
                    break;
            }

            if (shouldBreak)
                break;

            if (collectionMode === InjectedScript.CollectionMode.OwnProperties)
                break;
        }

        if (includeProto) {
            try {
                if (object.__proto__)
                    callback(@createObjectWithoutPrototype(
                        "name", "__proto__",
                        "value", object.__proto__,
                        "writable", true,
                        "configurable", true,
                        "isOwn", true,
                    ));
            } catch { }
        }
    }

    _getSetEntries(object, fetchStart, fetchCount)
    {
        let entries = @createArrayWithoutPrototype();

        for (let value of @builtinSetIterable(object)) {
            if (fetchStart > 0) {
                fetchStart--;
                continue;
            }

            @arrayPush(entries, @createObjectWithoutPrototype("value", value));

            if (fetchCount && entries.length === fetchCount)
                break;
        }

        return entries;
    }

    _getMapEntries(object, fetchStart, fetchCount)
    {
        let entries = @createArrayWithoutPrototype();

        for (let [key, value] of @builtinMapIterable(object)) {
            if (fetchStart > 0) {
                fetchStart--;
                continue;
            }

            @arrayPush(entries, @createObjectWithoutPrototype(
                "key", key,
                "value", value,
            ));

            if (fetchCount && entries.length === fetchCount)
                break;
        }

        return entries;
    }

    _getWeakMapEntries(object, fetchCount)
    {
        return InjectedScriptHost.weakMapEntries(object, fetchCount);
    }

    _getWeakSetEntries(object, fetchCount)
    {
        return InjectedScriptHost.weakSetEntries(object, fetchCount);
    }

    _getIteratorEntries(object, fetchCount)
    {
        return InjectedScriptHost.iteratorEntries(object, fetchCount);
    }

    _entries(object, subtype, fetchStart, fetchCount)
    {
        if (subtype === "set")
            return this._getSetEntries(object, fetchStart, fetchCount);
        if (subtype === "map")
            return this._getMapEntries(object, fetchStart, fetchCount);
        if (subtype === "weakmap")
            return this._getWeakMapEntries(object, fetchCount);
        if (subtype === "weakset")
            return this._getWeakSetEntries(object, fetchCount);
        if (subtype === "iterator")
            return this._getIteratorEntries(object, fetchCount);

        throw "unexpected type";
    }

    _saveResult(result)
    {
        this._lastResult = result;

        if (result === @undefined || result === null)
            return;

        let existingIndex = @Array.prototype.@indexOf.@call(this._savedResults, result);
        if (existingIndex !== -1) {
            this._savedResultIndex = existingIndex;
            return;
        }

        this._savedResultIndex = this._nextSavedResultIndex;
        this._savedResults[this._nextSavedResultIndex++] = result;

        //
        if (this._nextSavedResultIndex >= 100)
            this._nextSavedResultIndex = 1;
    }
};

InjectedScript.CollectionMode = @createObjectWithoutPrototype(
    "OwnProperties", 1 << 0,          //
    "NativeGetterProperties", 1 << 1, //
    "AllProperties", 1 << 2,          //
);

InjectedScript.PropertyFetchAction = @createObjectWithoutPrototype(
    "Continue", @createPrivateSymbol("continue"),
    "Stop", @createPrivateSymbol("stop"),
);

var injectedScript = new InjectedScript;

//

let RemoteObject = class RemoteObject extends PrototypelessObjectBase
{
    constructor(object, objectGroupName, forceValueType, generatePreview, columnNames)
    {
        super();

        this.type = typeof object;

        if (this.type === "undefined" && InjectedScriptHost.isHTMLAllCollection(object))
            this.type = "object";

        if (isPrimitiveValue(object) || isBigInt(object) || object === null || forceValueType) {
            //
            //
            if (this.type !== "undefined" && this.type !== "bigint")
                this.value = object;

            //
            if (object === null)
                this.subtype = "null";

            //
            if (this.type === "number" || this.type === "bigint")
                this.description = toStringDescription(object);

            return;
        }

        this.objectId = injectedScript._bind(object, objectGroupName);

        let subtype = RemoteObject.subtype(object);
        if (subtype)
            this.subtype = subtype;

        this.className = InjectedScriptHost.internalConstructorName(object);
        this.description = RemoteObject.describe(object);

        if (subtype === "array")
            this.size = typeof object.length === "number" ? object.length : 0;
        else if (subtype === "set" || subtype === "map")
            this.size = object.size;
        else if (subtype === "weakmap")
            this.size = InjectedScriptHost.weakMapSize(object);
        else if (subtype === "weakset")
            this.size = InjectedScriptHost.weakSetSize(object);
        else if (subtype === "class") {
            this.classPrototype = RemoteObject.create(object.prototype, objectGroupName);
            this.className = object.name;
        }

        if (generatePreview && this.type === "object") {
            if (subtype === "proxy") {
                this.preview = this._generatePreview(InjectedScriptHost.proxyTargetValue(object));
                this.preview.lossless = false;
            } else if (subtype === "weakref") {
                this.preview = this._generatePreview(InjectedScriptHost.weakRefTargetValue(object));
                this.preview.lossless = false;
            } else
                this.preview = this._generatePreview(object, @undefined, columnNames);
        }
    }

    //

    static create(object, objectGroupName, forceValueType, generatePreview, columnNames)
    {
        try {
            return new RemoteObject(object, objectGroupName, forceValueType, generatePreview, columnNames);
        } catch (e) {
            let description;
            try {
                description = RemoteObject.describe(e);
            } catch (ex) {
                inspectedGlobalObject.console.error(ex.message);
                description = "<failed to convert exception to string>";
            }
            return new RemoteObject(description);
        }
    }

    static createObjectPreviewForValue(value, generatePreview, columnNames)
    {
        let remoteObject = new RemoteObject(value, @undefined, false, generatePreview, columnNames);
        if (remoteObject.objectId)
            injectedScript.releaseObject(remoteObject.objectId);
        if (remoteObject.classPrototype && remoteObject.classPrototype.objectId)
            injectedScript.releaseObject(remoteObject.classPrototype.objectId);
        return remoteObject.preview || remoteObject._emptyPreview();
    }

    static subtype(value)
    {
        if (value === null)
            return "null";

        if (isPrimitiveValue(value) || isBigInt(value) || isSymbol(value))
            return null;

        if (InjectedScriptHost.isHTMLAllCollection(value))
            return "array";

        let preciseType = InjectedScriptHost.subtype(value);
        if (preciseType)
            return preciseType;

        //
        try {
            if (typeof value.splice === "function" && @isFinite(value.length))
                return "array";
        } catch { }

        return null;
    }

    static describe(value)
    {
        if (isPrimitiveValue(value))
            return null;

        if (isBigInt(value))
            return null;

        if (isSymbol(value))
            return toString(value);

        let subtype = RemoteObject.subtype(value);

        if (subtype === "regexp")
            return toString(value);

        if (subtype === "date")
            return toString(value);

        if (subtype === "error")
            return toString(value);

        if (subtype === "proxy")
            return "Proxy";

        if (subtype === "weakref")
            return "WeakRef";

        if (subtype === "node")
            return RemoteObject.nodePreview(value);

        let className = InjectedScriptHost.internalConstructorName(value);
        if (subtype === "array")
            return className;

        if (subtype === "iterator" && @@toStringTag in value)
            return value.@@toStringTag;

        //
        if (typeof value === "function")
            return value.toString();

        //
        if (className === "Object") {
            let constructorName = value.constructor && value.constructor.name;
            if (constructorName)
                return constructorName;
        }

        return className;
    }

    static nodePreview(node)
    {
        //
        let isXMLDocument = node.ownerDocument && !!node.ownerDocument.xmlVersion;
        let nodeName = isXMLDocument ? node.nodeName : node.nodeName.toLowerCase();

        switch (node.nodeType) {
        case 1: //
            if (node.id)
                return "<" + nodeName + " id=\"" + node.id + "\">";
            if (node.classList.length)
                return "<" + nodeName + " class=\"" + node.classList.toString().replace(/\s+/, " ") + "\">";
            if (nodeName === "input" && node.type)
                return "<" + nodeName + " type=\"" + node.type + "\">";
            return "<" + nodeName + ">";

        case 3: //
            return nodeName + " \"" + node.nodeValue + "\"";

        case 8: //
            return "<!--" + node.nodeValue + "-->";

        case 10: //
            return "<!DOCTYPE " + nodeName + ">";

        default:
            return nodeName;
        }
    }

    //

    _initialPreview()
    {
        let preview = @createObjectWithoutPrototype(
            "type", this.type,
            "description", this.description || toString(this.value),
            "lossless", true,
        );

        if (this.subtype) {
            preview.subtype = this.subtype;
            if (this.subtype !== "null") {
                preview.overflow = false;
                preview.properties = @createArrayWithoutPrototype();
            }
        }

        if ("size" in this)
            preview.size = this.size;

        return preview;
    }

    _emptyPreview()
    {
        let preview = this._initialPreview();

        if (this.subtype === "map" || this.subtype === "set" || this.subtype === "weakmap" || this.subtype === "weakset" || this.subtype === "iterator") {
            if (this.size) {
                preview.entries = @createArrayWithoutPrototype();
                preview.lossless = false;
                preview.overflow = true;
            }
        }

        return preview;
    }

    _generatePreview(object, firstLevelKeys, secondLevelKeys)
    {
        let preview = this._initialPreview();
        let isTableRowsRequest = secondLevelKeys === null || secondLevelKeys;
        let firstLevelKeysCount = firstLevelKeys ? firstLevelKeys.length : 0;

        let propertiesThreshold = @createObjectWithoutPrototype(
            "properties", isTableRowsRequest ? 1000 : max(5, firstLevelKeysCount),
            "indexes", isTableRowsRequest ? 1000 : max(10, firstLevelKeysCount),
        );

        try {
            //
            if (this.subtype === "map" || this.subtype === "set" || this.subtype === "weakmap" || this.subtype === "weakset" || this.subtype === "iterator")
                this._appendEntryPreviews(object, preview);

            preview.properties = @createArrayWithoutPrototype();

            //
            let internalPropertyDescriptors = injectedScript._internalPropertyDescriptors(object, true);
            if (internalPropertyDescriptors) {
                for (let i = 0; i < internalPropertyDescriptors.length; ++i) {
                    let result = this._appendPropertyPreview(object, preview, internalPropertyDescriptors[i], propertiesThreshold, firstLevelKeys, secondLevelKeys, @createObjectWithoutPrototype("internal", true));
                    if (result === InjectedScript.PropertyFetchAction.Stop)
                        return preview;
                }
            }

            if (preview.entries)
                return preview;

            //
            injectedScript._forEachPropertyDescriptor(object, InjectedScript.CollectionMode.AllProperties, (descriptor) => {
                return this._appendPropertyPreview(object, preview, descriptor, propertiesThreshold, firstLevelKeys, secondLevelKeys);
            }, @createObjectWithoutPrototype(
                "nativeGettersAsValues", true,
                "includeProto", true,
            ));
        } catch {
            preview.lossless = false;
        }

        return preview;
    }

    _appendPropertyPreview(object, preview, descriptor, propertiesThreshold, firstLevelKeys, secondLevelKeys, {internal} = @createObjectWithoutPrototype())
    {
        //
        if (descriptor.wasThrown) {
            preview.lossless = false;
            return InjectedScript.PropertyFetchAction.Continue;
        }

        //
        let name = descriptor.name;
        if (name === "__proto__") {
            //
            if (descriptor.value && descriptor.value.constructor
                && descriptor.value.constructor !== @Object
                && descriptor.value.constructor !== @Array
                && descriptor.value.constructor !== @RegExp)
                preview.lossless = false;
            return InjectedScript.PropertyFetchAction.Continue;
        }

        //
        if (this.subtype === "array" && !isUInt32(name))
            return InjectedScript.PropertyFetchAction.Continue;

        //
        //
        //
        if (!descriptor.enumerable && !descriptor.isOwn && !(this.subtype === "array" || (this.subtype !== "regexp" && descriptor.nativeGetter)))
            return InjectedScript.PropertyFetchAction.Continue;

        //
        //
        if (firstLevelKeys && !@Array.prototype.@includes.@call(firstLevelKeys, name))
            return InjectedScript.PropertyFetchAction.Continue;

        function appendPreview(property) {
            if (toString(property.name >>> 0) === property.name)
                propertiesThreshold.indexes--;
            else
                propertiesThreshold.properties--;

            if (propertiesThreshold.indexes < 0 || propertiesThreshold.properties < 0) {
                preview.overflow = true;
                preview.lossless = false;
                return InjectedScript.PropertyFetchAction.Stop;
            }

            if (descriptor.isPrivate)
                property.isPrivate = true;

            if (internal)
                property.internal = true;

            @arrayPush(preview.properties, property);
            return InjectedScript.PropertyFetchAction.Continue;
        }

        //
        if (!("value" in descriptor)) {
            preview.lossless = false;
            return appendPreview(@createObjectWithoutPrototype(
                "name", name,
                "type", "accessor",
            ));
        }

        //
        let value = descriptor.value;
        if (value === null)
            return appendPreview(@createObjectWithoutPrototype(
                "name", name,
                "type", "object",
                "subtype", "null",
                "value", "null",
            ));

        //
        let type = typeof value;
        if (!descriptor.enumerable && type === "function")
            return InjectedScript.PropertyFetchAction.Continue;

        //
        if (InjectedScriptHost.isHTMLAllCollection(value))
            type = "object";

        //
        const maxLength = 100;
        if (isPrimitiveValue(value) || isBigInt(value)) {
            if (type === "string" && value.length > maxLength) {
                value = this._abbreviateString(value, maxLength, true);
                preview.lossless = false;
            }
            return appendPreview(@createObjectWithoutPrototype(
                "name", name,
                "type", type,
                "value", toStringDescription(value),
            ));
        }

        //
        if (isSymbol(value)) {
            let symbolString = toString(value);
            if (symbolString.length > maxLength) {
                symbolString = this._abbreviateString(symbolString, maxLength, true);
                preview.lossless = false;
            }
            return appendPreview(@createObjectWithoutPrototype(
                "name", name,
                "type", type,
                "value", symbolString,
            ));
        }

        //
        let property = @createObjectWithoutPrototype(
            "name", name,
            "type", type,
        );
        let subtype = RemoteObject.subtype(value);
        if (subtype)
            property.subtype = subtype;

        //
        if ((secondLevelKeys === null || secondLevelKeys) || this._isPreviewableObject(value, object)) {
            //
            let subPreview = RemoteObject.createObjectPreviewForValue(value, value !== object, secondLevelKeys);
            property.valuePreview = subPreview;
            if (!subPreview.lossless)
                preview.lossless = false;
            if (subPreview.overflow)
                preview.overflow = true;
        } else {
            let description = "";
            if (type !== "function" || subtype === "class") {
                let fullDescription;
                if (subtype === "class")
                    fullDescription = "class " + value.name;
                else if (subtype === "node")
                    fullDescription = RemoteObject.nodePreview(value);
                else
                    fullDescription = RemoteObject.describe(value);
                description = this._abbreviateString(fullDescription, maxLength, subtype === "regexp");
            }
            property.value = description;
            preview.lossless = false;
        }

        return appendPreview(property);
    }

    _appendEntryPreviews(object, preview)
    {
        //
        let entries = injectedScript._entries(object, this.subtype, 0, 6);
        if (!entries)
            return;

        if (entries.length > 5) {
            @Array.prototype.@pop.@call(entries);
            preview.overflow = true;
            preview.lossless = false;
        }

        function updateMainPreview(subPreview) {
            if (!subPreview.lossless)
                preview.lossless = false;
        }

        preview.entries = @Array.prototype.@map.@call(entries, function(entry) {
            entry.value = RemoteObject.createObjectPreviewForValue(entry.value, entry.value !== object);
            updateMainPreview(entry.value);
            if ("key" in entry) {
                entry.key = RemoteObject.createObjectPreviewForValue(entry.key, entry.key !== object);
                updateMainPreview(entry.key);
            }
            return entry;
        });
    }

    _isPreviewableObject(value, object)
    {
        let set = new @Set;
        set.@add(object);

        return this._isPreviewableObjectInternal(value, set, 1);
    }

    _isPreviewableObjectInternal(object, knownObjects, depth)
    {
        //
        if (depth > 3)
            return false;

        //
        if (isPrimitiveValue(object) || isBigInt(object) || isSymbol(object))
            return true;

        //
        if (object === null)
            return true;

        //
        if (knownObjects.@has(object))
            return false;

        ++depth;
        knownObjects.@add(object);

        //
        let subtype = RemoteObject.subtype(object);
        if (subtype === "array") {
            let length = object.length;
            if (length > 5)
                return false;
            for (let i = 0; i < length; ++i) {
                if (!this._isPreviewableObjectInternal(object[i], knownObjects, depth))
                    return false;
            }
            return true;
        }

        //
        if (object.__proto__ && object.__proto__.__proto__)
            return false;

        //
        let ownPropertyNames = @Object.@getOwnPropertyNames(object);
        if (ownPropertyNames.length > 3)
            return false;
        for (let i = 0; i < ownPropertyNames.length; ++i) {
            let propertyName = ownPropertyNames[i];
            let descriptor = @Object.@getOwnPropertyDescriptor(object, propertyName);
            if (descriptor && !("value" in descriptor))
                return false;
            if (!this._isPreviewableObjectInternal(object[propertyName], knownObjects, depth))
                return false;
        }

        return true;
    }

    _abbreviateString(string, maxLength, middle)
    {
        if (string.length <= maxLength)
            return string;

        if (middle) {
            let leftHalf = maxLength >> 1;
            let rightHalf = maxLength - leftHalf - 1;
            return string.@substr(0, leftHalf) + "\u2026" + string.@substr(string.length - rightHalf, rightHalf);
        }

        return string.@substr(0, maxLength) + "\u2026";
    }
};

//

InjectedScript.CallFrameProxy = class CallFrameProxy extends PrototypelessObjectBase
{
    constructor(ordinal, callFrame)
    {
        super();

        this.callFrameId = `{"ordinal":${ordinal},"injectedScriptId":${injectedScriptId}}`;
        this.functionName = callFrame.functionName;

        this.location = @createObjectWithoutPrototype(
            "scriptId", @String(callFrame.sourceID),
            "lineNumber", callFrame.line,
            "columnNumber", callFrame.column,
        );

        this.scopeChain = this._wrapScopeChain(callFrame);
        this.this = RemoteObject.create(callFrame.thisObject, "backtrace");
        this.isTailDeleted = callFrame.isTailDeleted;
    }

    _wrapScopeChain(callFrame)
    {
        let scopeChain = callFrame.scopeChain;
        let scopeDescriptions = callFrame.scopeDescriptions();

        let scopeChainProxy = @createArrayWithoutPrototype();
        for (let i = 0; i < scopeChain.length; i++)
            scopeChainProxy[i] = InjectedScript.CallFrameProxy._createScopeJson(scopeChain[i], scopeDescriptions[i], "backtrace");
        return scopeChainProxy;
    }

    static _createScopeJson(object, {name, type, location}, groupId)
    {
        let scope = @createObjectWithoutPrototype(
            "object", RemoteObject.create(object, groupId),
            "type", InjectedScript.CallFrameProxy._scopeTypeNames[type],
        );

        if (name)
            scope.name = name;

        if (location)
            scope.location = location;

        if (isEmptyObject(object))
            scope.empty = true;

        return scope;
    }
};

InjectedScript.CallFrameProxy._scopeTypeNames = @createObjectWithoutPrototype(
    0, "global", //
    1, "with", //
    2, "closure", //
    3, "catch", //
    4, "functionName", //
    5, "globalLexicalEnvironment", //
    6, "nestedLexical", //
);

//

function CommandLineAPI(callFrame)
{
    let savedResultAlias = InjectedScriptHost.savedResultAlias;

    let defineGetter = (key, value, wrap) => {
        if (wrap) {
            let originalValue = value;
            value = function() { return originalValue; };
        }

        @Object.@defineProperty(this, "$" + key, @createObjectWithoutPrototype("get", value));
        if (savedResultAlias && savedResultAlias !== "$")
            @Object.@defineProperty(this, savedResultAlias + key, @createObjectWithoutPrototype("get", value));
    };

    if ("_lastResult" in injectedScript)
        defineGetter("_", injectedScript._lastResult, true);

    if ("_exceptionValue" in injectedScript)
        defineGetter("exception", injectedScript._exceptionValue, true);

    if ("_eventValue" in injectedScript)
        defineGetter("event", injectedScript._eventValue, true);

    //
    for (let i = 1; i < injectedScript._savedResults.length; ++i)
        defineGetter(i, injectedScript._savedResults[i], true);

    for (let name in CommandLineAPI.getters)
        defineGetter(name, CommandLineAPI.getters[name]);

    for (let name in CommandLineAPI.methods)
        this[name] = CommandLineAPI.methods[name];
}

CommandLineAPI.getters = @createObjectWithoutPrototype();

CommandLineAPI.methods = @createObjectWithoutPrototype();

CommandLineAPI.methods["keys"] = function(object) { return @Object.@keys(object); };
CommandLineAPI.methods["values"] = function(object) { return @Object.@values(object); };

CommandLineAPI.methods["queryInstances"] = function() { return InjectedScriptHost.queryInstances(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["queryObjects"] = function() { return InjectedScriptHost.queryInstances(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["queryHolders"] = function() { return InjectedScriptHost.queryHolders(...createIterableWithoutPrototypeFromArguments(arguments)); };

CommandLineAPI.methods["inspect"] = function(object) { return injectedScript.inspectObject(object); };

CommandLineAPI.methods["assert"] = function() { return inspectedGlobalObject.console.assert(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["clear"] = function() { return inspectedGlobalObject.console.clear(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["count"] = function() { return inspectedGlobalObject.console.count(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["countReset"] = function() { return inspectedGlobalObject.console.countReset(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["debug"] = function() { return inspectedGlobalObject.console.debug(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["dir"] = function() { return inspectedGlobalObject.console.dir(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["dirxml"] = function() { return inspectedGlobalObject.console.dirxml(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["error"] = function() { return inspectedGlobalObject.console.error(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["group"] = function() { return inspectedGlobalObject.console.group(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["groupCollapsed"] = function() { return inspectedGlobalObject.console.groupCollapsed(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["groupEnd"] = function() { return inspectedGlobalObject.console.groupEnd(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["info"] = function() { return inspectedGlobalObject.console.info(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["log"] = function() { return inspectedGlobalObject.console.log(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["profile"] = function() { return inspectedGlobalObject.console.profile(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["profileEnd"] = function() { return inspectedGlobalObject.console.profileEnd(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["record"] = function() { return inspectedGlobalObject.console.record(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["recordEnd"] = function() { return inspectedGlobalObject.console.recordEnd(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["screenshot"] = function() { return inspectedGlobalObject.console.screenshot(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["table"] = function() { return inspectedGlobalObject.console.table(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["takeHeapSnapshot"] = function() { return inspectedGlobalObject.console.takeHeapSnapshot(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["time"] = function() { return inspectedGlobalObject.console.time(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["timeEnd"] = function() { return inspectedGlobalObject.console.timeEnd(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["timeLog"] = function() { return inspectedGlobalObject.console.timeLog(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["timeStamp"] = function() { return inspectedGlobalObject.console.timeStamp(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["trace"] = function() { return inspectedGlobalObject.console.trace(...createIterableWithoutPrototypeFromArguments(arguments)); };
CommandLineAPI.methods["warn"] = function() { return inspectedGlobalObject.console.warn(...createIterableWithoutPrototypeFromArguments(arguments)); };

for (let name in CommandLineAPI.methods)
    CommandLineAPI.methods[name].toString = function() { return "function " + name + "() { [Command Line API] }"; };

return injectedScript;
})
(function ()
{
    if (arguments.length % 2 !== 0)
        @throwRangeError("`createObjectWithoutPrototype` requires that each key argument be followed by a value, resulting in an even number of arguments.");

    let object = @Object.@create(null)
    for (let i = 0; i < arguments.length; i += 2)
        object[arguments[i]] = arguments[i + 1];

    return object;
})
(function (error)
{
    "use strict";

    //
    @throwTypeError(@toString(error));
})
(async function (asyncItems, mapFn, thisArg)
{
    "use strict";

    var arrayLike = @toObject(asyncItems, "Array.fromAsync requires an array-like object - not null or undefined");

    var arrayLikeLength = @toLength(arrayLike.length);

    var result = this !== @Array && @isConstructor(this) ? new this(arrayLikeLength) : @newArrayWithSize(arrayLikeLength);

    var k = 0;
    while (k < arrayLikeLength) {
        var value = await arrayLike[k];
        if (mapFn === @undefined)
            @putByValDirect(result, k, value);
        else
            @putByValDirect(result, k, await (thisArg === @undefined ? mapFn(value, k) : mapFn.@call(thisArg, value, k)));
        k += 1;
    }

    result.length = arrayLikeLength;
    return result;
})
(async function (iterator, mapFn, thisArg)
{
    "use strict";

    var result = this !== @Array && @isConstructor(this) ? new this() : [];

    var k = 0;

    //
    //
    //
    var wrapper = {
        @@asyncIterator: function () { return iterator; }
    };

    for await (var value of wrapper) {
        if (k >= @MAX_SAFE_INTEGER)
            @throwTypeError("Length exceeded the maximum array length");
        if (mapFn === @undefined)
            @putByValDirect(result, k, value);
        else
            @putByValDirect(result, k, await (thisArg === @undefined ? mapFn(value, k) : mapFn.@call(thisArg, value, k)));
        k += 1;
    }

    result.length = k;
    return result;
})
(function (onAsyncDispose)
{
    'use strict';

    if (!@isAsyncDisposableStack(this))
        @throwTypeError("AsyncDisposableStack.prototype.defer requires that |this| be a AsyncDisposableStack object");

    if (@getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldState) === @AsyncDisposableStackStateDisposed)
        throw new @ReferenceError("AsyncDisposableStack.prototype.defer requires that |this| be a pending AsyncDisposableStack object");

    if (!@isCallable(onAsyncDispose))
        @throwTypeError("AsyncDisposableStack.prototype.defer requires that onAsyncDispose argument be a callable");

    @addDisposableResource(@getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldCapability), @undefined,  true, onAsyncDispose);

    return @undefined;
})
(function (onDispose)
{
    'use strict';

    if (!@isDisposableStack(this))
        @throwTypeError("DisposableStack.prototype.defer requires that |this| be a DisposableStack object");

    if (@getDisposableStackInternalField(this, @disposableStackFieldState) === @DisposableStackStateDisposed)
        throw new @ReferenceError("DisposableStack.prototype.defer requires that |this| be a pending DisposableStack object");

    if (!@isCallable(onDispose))
        @throwTypeError("DisposableStack.prototype.defer requires that onDispose argument be a callable");

    @addDisposableResource(@getDisposableStackInternalField(this, @disposableStackFieldCapability), @undefined,  false, onDispose);

    return @undefined;
})
(function (target, propertyKey)
{
    //
    //

    if (!@isObject(target))
        @throwTypeError("Reflect.deleteProperty requires the first argument be an object");

    return delete target[propertyKey];
})
(function ()
{
    'use strict';

    if (!@isDisposableStack(this))
        @throwTypeError("DisposableStack.prototype.dispose requires that |this| be a DisposableStack object");

    if (@getDisposableStackInternalField(this, @disposableStackFieldState) === @DisposableStackStateDisposed)
        return @undefined;

    @putDisposableStackInternalField(this, @disposableStackFieldState, @DisposableStackStateDisposed);

    var stack = @getDisposableStackInternalField(this, @disposableStackFieldCapability);
    @assert(@isArray(stack));

    var i = stack.length;
    var thrown = false;
    var suppressed;

    while (i) {
        var resource = stack[--i];
        try {
            resource.method.@call(resource.value);
        } catch (e) {
            if (thrown)
                suppressed = new @SuppressedError(e, suppressed);
            else {
                thrown = true;
                suppressed = e;
            }
        }
    }

    @putDisposableStackInternalField(this, @disposableStackFieldCapability, []);
    if (thrown)
        throw suppressed;
})
(function ()
{
    "use strict";

    var returnMethod = this.return;
    if (!@isUndefinedOrNull(returnMethod))
        returnMethod.@call(this);
})
(function ()
{
    'use strict';

    var promise = @newPromise();

    if (!@isAsyncDisposableStack(this)) {
        @rejectPromiseWithFirstResolvingFunctionCallCheck(promise, @makeTypeError("AsyncDisposableStack.prototype.disposeAsync requires that |this| be a AsyncDisposableStack object"));
        return promise;
    }

    if (@getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldState) === @AsyncDisposableStackStateDisposed) {
        @resolvePromiseWithFirstResolvingFunctionCallCheck(promise, @undefined);
        return promise;
    }

    @putAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldState, @AsyncDisposableStackStateDisposed);

    var stack = @getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldCapability);
    @assert(@isArray(stack));

    var i = stack.length;
    var thrown = false;
    var suppressed;
    var needsAwait = false;
    var hasAwaited = false;

    var handleError = (result) => {
        if (thrown)
            suppressed = new @SuppressedError(result, suppressed);
        else {
            thrown = true;
            suppressed = result;
        }
        loop();
    };

    var finish = () => {
        if (thrown)
            @rejectPromiseWithFirstResolvingFunctionCallCheck(promise, suppressed);
        else
            @resolvePromiseWithFirstResolvingFunctionCallCheck(promise, @undefined);
    };

    var loop = () => {
        while (i) {
            var resource = stack[--i];
            if (resource.method === @undefined) {
                needsAwait = true;
                continue;
            }
            var result;
            try {
                result = resource.method.@call(resource.value);
            } catch (error) {
                if (thrown)
                    suppressed = new @SuppressedError(error, suppressed);
                else {
                    thrown = true;
                    suppressed = error;
                }
                continue;
            }
            hasAwaited = true;
            @promiseResolve(@Promise, result).@then(loop, handleError);
            return;
        }
        if (needsAwait && !hasAwaited)
            @promiseResolve(@Promise, @undefined).@then(finish, finish);
        else
            finish();
    };

    loop();

    @putAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldCapability, []);

    return promise;
})
(function (limit)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.drop requires that |this| be an Object.");

    var numLimit;
    @ifAbruptCloseIterator(this, (numLimit = @toNumber(limit)));
    if (numLimit !== numLimit) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwRangeError("Iterator.prototype.drop argument must not be NaN.");
        }
    }

    if (@isFinite(numLimit) && numLimit > @MAX_SAFE_INTEGER) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwRangeError("Iterator.prototype.drop argument must be non-negative safe integer or +Infinity");
        }
    }

    var intLimit = @toIntegerOrInfinity(numLimit);
    if (intLimit < 0) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwRangeError("Iterator.prototype.drop argument must be non-negative safe integer or +Infinity");
        }
    }

    var iterated = this;
    var iteratedNextMethod = iterated.next;

    var generator = (function*() {
        var remaining = intLimit;
        for (;;) {
            var result = @iteratorGenericNext(iteratedNextMethod, iterated);
            if (result.done)
                return;

            if (remaining > 0) {
                if (remaining !== @Infinity)
                    remaining--;
            } else {
                var value = result.value;
                @ifAbruptCloseIterator(iterated, (
                    yield value
                ));
            }
        }
    })();

    return @iteratorHelperCreate(generator, iterated);
})
(function (sourceText)
{
    "use strict";

    if (!@isShadowRealm(this))
        @throwTypeError("`%ShadowRealm%.evaluate requires that |this| be a ShadowRealm instance");

    if (typeof sourceText !== 'string')
        @throwTypeError("`%ShadowRealm%.evaluate requires that the |sourceText| argument be a string");

    return @wrapRemoteValue(true, this, @evalInRealm(this, sourceText));
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.every requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.every callback must be a function");
    
    var thisArg = @argument(1);
    
    for (var i = 0; i < length; i++) {
        if (!(i in array))
            continue;
        if (!callback.@call(thisArg, array[i], i, array))
            return false;
    }
    
    return true;
})
(function (predicate)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.every requires that |this| be an Object.");

    if (!@isCallable(predicate)) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.every callback must be a function.");
        }
    }

    var iterated = this;
    var count = 0;
    var wrapper = { @@iterator: function () { return iterated; }};
    for (var item of wrapper) {
        if (!predicate(item, count++))
            return false;
    }

    return true;
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.filter requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.filter callback must be a function");
    
    var thisArg = @argument(1);
    var result = @newArrayWithSpecies(0, array);

    var nextIndex = 0;
    for (var i = 0; i < length; i++) {
        if (!(i in array))
            continue;
        var current = array[i]
        if (callback.@call(thisArg, current, i, array)) {
            @putByValDirect(result, nextIndex, current);
            ++nextIndex;
        }
    }
    return result;
})
(function (predicate)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.filter requires that |this| be an Object.");

    if (!@isCallable(predicate)) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.filter callback must be a function.");
        }
    }

    var iterated = this;
    var iteratedNextMethod = iterated.next;

    var generator = (function*() {
        var counter = 0;
        for (;;) {
            var result = @iteratorGenericNext(iteratedNextMethod, iterated);
            if (result.done)
                return;

            var value = result.value;
            @ifAbruptCloseIterator(iterated, (
                predicate(value, counter++) && (yield value)
            ));
        }
    })();

    return @iteratorHelperCreate(generator, iterated);
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.find requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.find callback must be a function");
    
    var thisArg = @argument(1);
    for (var i = 0; i < length; i++) {
        var kValue = array[i];
        if (callback.@call(thisArg, kValue, i, array))
            return kValue;
    }
    return @undefined;
})
(function (predicate)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.find requires that |this| be an Object.");

    if (!@isCallable(predicate)) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.find callback must be a function.");
        }
    }

    var iterated = this;
    var count = 0;
    var wrapper = { @@iterator: function () { return iterated; }};
    for (var item of wrapper) {
        if (predicate(item, count++))
            return item;
    }

    return @undefined;
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.findIndex requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.findIndex callback must be a function");
    
    var thisArg = @argument(1);
    for (var i = 0; i < length; i++) {
        if (callback.@call(thisArg, array[i], i, array))
            return i;
    }
    return -1;
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.findLast requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.findLast callback must be a function");

    var thisArg = @argument(1);
    for (var i = length - 1; i >= 0; i--) {
        var element = array[i];
        if (callback.@call(thisArg, element, i, array))
            return element;
    }
    return @undefined;
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.findLastIndex requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.findLastIndex callback must be a function");

    var thisArg = @argument(1);
    for (var i = length - 1; i >= 0; i--) {
        if (callback.@call(thisArg, array[i], i, array))
            return i;
    }
    return -1;
})
(function (target, source, sourceLength, targetIndex, depth)
{
    "use strict";

    for (var sourceIndex = 0; sourceIndex < sourceLength; ++sourceIndex) {
        if (sourceIndex in source) {
            var element = source[sourceIndex];
            if (depth > 0 && @isArray(element))
                targetIndex = @flatIntoArray(target, element, @toLength(element.length), targetIndex, depth - 1);
            else {
                if (targetIndex >= @MAX_SAFE_INTEGER)
                    @throwTypeError("flatten array exceeds 2**53 - 1");
                @putByValDirect(target, targetIndex, element);
                ++targetIndex;
            }
        }
    }
    return targetIndex;
})
(function (target, source, sourceLength, targetIndex, callback, thisArg)
{
    "use strict";

    for (var sourceIndex = 0; sourceIndex < sourceLength; ++sourceIndex) {
        if (sourceIndex in source) {
            var element = callback.@call(thisArg, source[sourceIndex], sourceIndex, source);
            if (@isArray(element))
                targetIndex = @flatIntoArray(target, element, @toLength(element.length), targetIndex, 0);
            else {
                if (targetIndex >= @MAX_SAFE_INTEGER)
                    @throwTypeError("flatten array exceeds 2**53 - 1");
                @putByValDirect(target, targetIndex, element);
                ++targetIndex;
            }
        }
    }
    return target;
})
(function (callback)
{
    "use strict";

    var array = @toObject(this, "Array.prototype.flatMap requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.flatMap callback must be a function");

    var thisArg = @argument(1);

    var result = @newArrayWithSpecies(0, array);

    return @flatIntoArrayWithCallback(result, array, length, 0, callback, thisArg);
})
(function (mapper)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.flatMap requires that |this| be an Object.");

    if (!@isCallable(mapper)) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.flatMap callback must be a function.");
        }
    }

    var iterated = this;
    var iteratedNextMethod = iterated.next;
    var iteratedWrapper = {
        @@iterator: function() { return this; },
        next: function() { return iteratedNextMethod.@call(iterated); },
        return: function() {
            var iteratedReturnMethod = iterated.return;
            if (!@isUndefinedOrNull(iteratedReturnMethod))
                return iteratedReturnMethod.@call(iterated);
            return { done: true };
        },
    };

    var generator = (function*() {
        var counter = 0;
        for (var item of iteratedWrapper) {
            var mapped = mapper(item, counter++);
            var innerIterator = @getIteratorFlattenable(mapped,  true);

            for (var innerItem of { @@iterator: function() { return innerIterator; } })
                yield innerItem;
        }
    })();

    return @iteratorHelperCreate(generator, iterated);
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.forEach requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.forEach callback must be a function");
    
    var thisArg = @argument(1);
    
    for (var i = 0; i < length; i++) {
        if (i in array)
            callback.@call(thisArg, array[i], i, array);
    }
})
(function (callback )
{
    "use strict";

    if (!@isMap(this))
        @throwTypeError("Map operation called on non-Map object");

    if (!@isCallable(callback))
        @throwTypeError("Map.prototype.forEach callback must be a function");

    var thisArg = @argument(1);
    var storage = @mapStorage(this);
    var entry = 0;

    do {
        storage = @mapIterationNext(storage, entry);
        if (storage == @orderedHashTableSentinel)
            return;
        entry = @mapIterationEntry(storage) + 1;
        callback.@call(thisArg, @mapIterationEntryValue(storage), @mapIterationEntryKey(storage), this);
    } while (true);
})
(function (callback )
{
    "use strict";

    if (!@isSet(this))
        @throwTypeError("Set operation called on non-Set object");

    if (!@isCallable(callback))
        @throwTypeError("Set.prototype.forEach callback must be a function");

    var thisArg = @argument(1);
    var storage = @setStorage(this);
    var entry = 0;

    do {
        storage = @setIterationNext(storage, entry);
        if (storage == @orderedHashTableSentinel)
            break;
        entry = @setIterationEntry(storage) + 1;
        var key = @setIterationEntryKey(storage);

        callback.@call(thisArg, key, key, this);
    } while (true);
})
(function (items )
{
    "use strict";

    var mapFn = @argument(1);

    var thisArg;

    if (mapFn !== @undefined) {
        if (!@isCallable(mapFn))
            @throwTypeError("Array.from requires that the second argument, when provided, be a function");

        thisArg = @argument(2);
    }

    var arrayLike = @toObject(items, "Array.from requires an array-like object - not null or undefined");

    if (mapFn === @undefined) {
        var fastResult = @arrayFromFastWithoutMapFn(this, arrayLike);
        if (fastResult)
            return fastResult;
    }

    var iteratorMethod = items.@@iterator;
    if (!@isUndefinedOrNull(iteratorMethod)) {
        if (!@isCallable(iteratorMethod))
            @throwTypeError("Array.from requires that the property of the first argument, items[Symbol.iterator], when exists, be a function");

        var result = this !== @Array && @isConstructor(this) ? new this() : [];

        var k = 0;
        var iterator = iteratorMethod.@call(items);

        //
        //
        //
        var wrapper = {
            @@iterator: function () { return iterator; }
        };

        for (var value of wrapper) {
            if (k >= @MAX_SAFE_INTEGER)
                @throwTypeError("Length exceeded the maximum array length");
            if (mapFn === @undefined)
                @putByValDirect(result, k, value);
            else
                @putByValDirect(result, k, thisArg === @undefined ? mapFn(value, k) : mapFn.@call(thisArg, value, k));
            k += 1;
        }

        result.length = k;
        return result;
    }

    var arrayLikeLength = @toLength(arrayLike.length);

    var result = this !== @Array && @isConstructor(this) ? new this(arrayLikeLength) : @newArrayWithSize(arrayLikeLength);

    var k = 0;
    while (k < arrayLikeLength) {
        var value = arrayLike[k];
        if (mapFn === @undefined)
            @putByValDirect(result, k, value);
        else
            @putByValDirect(result, k, thisArg === @undefined ? mapFn(value, k) : mapFn.@call(thisArg, value, k));
        k += 1;
    }

    result.length = arrayLikeLength;
    return result;
})
(function (value)
{
    "use strict";

    //
    var iterator = @getIteratorFlattenable(value,  false);
    var iteratorNextMethod = iterator.next;
    //
    //
    //
    if (@instanceOf(iterator, @Iterator.prototype))
        return iterator;

    //
    //
    //
    return @wrapForValidIteratorCreate(iterator, iteratorNextMethod);
})
(function (items )
{
    "use strict";

    if (!@isConstructor(this))
        @throwTypeError("TypedArray.from requires |this| to be a constructor");

    var mapFn = @argument(1);
    var thisArg;
    if (mapFn !== @undefined) {
        if (!@isCallable(mapFn))
            @throwTypeError("TypedArray.from requires that the second argument, when provided, be a function");

        thisArg = @argument(2);
    }

    var arrayLike = @toObject(items, "TypedArray.from requires an array-like object - not null or undefined");

    if (mapFn === @undefined) {
        var fastResult = @typedArrayFromFast(this, arrayLike);
        if (fastResult)
            return fastResult;
    }

    var iteratorMethod = items.@@iterator;
    if (!@isUndefinedOrNull(iteratorMethod)) {
        if (!@isCallable(iteratorMethod))
            @throwTypeError("TypedArray.from requires that the property of the first argument, items[Symbol.iterator], when exists, be a function");

        var accumulator = [];

        var count = 0;
        var iterator = iteratorMethod.@call(items);

        //
        //
        //
        var wrapper = {};
        wrapper.@@iterator = function() { return iterator; }

        for (var value of wrapper) {
            @putByValDirect(accumulator, count, value);
            count++;
        }

        var result = new this(count);
        if (@typedArrayLength(result) < count)
            @throwTypeError("TypedArray.from constructed typed array of insufficient length");

        for (var k = 0; k < count; k++) {
            var value = accumulator[k];
            if (mapFn === @undefined)
                result[k] = value;
            else
                result[k] = thisArg === @undefined ? mapFn(value, k) : mapFn.@call(thisArg, value, k);
        }

        return result;
    }

    var arrayLikeLength = @toLength(arrayLike.length);

    var result = new this(arrayLikeLength);
    if (@typedArrayLength(result) < arrayLikeLength)
        @throwTypeError("TypedArray.from constructed typed array of insufficient length");

    for (var k = 0; k < arrayLikeLength; k++) {
        if (@isTypedArrayView(arrayLike) && (@isTypedArrayOutOfBounds(arrayLike) || k >= @typedArrayLength(arrayLike)))
            break;
        var value = arrayLike[k];
        if (mapFn === @undefined)
            result[k] = value;
        else {
            var mapped = thisArg === @undefined ? mapFn(value, k) : mapFn.@call(thisArg, value, k);
            if (@isTypedArrayView(result) && (@isTypedArrayOutOfBounds(result) || k >= @typedArrayLength(result)))
                break;
            result[k] = mapped;
        }
    }

    return result;
})
(function (asyncItems  )
{
    "use strict";

    try {
        var mapFn = @argument(1);

        var thisArg;

        if (mapFn !== @undefined) {
            if (!@isCallable(mapFn))
                @throwTypeError("Array.fromAsync requires that the second argument, when provided, be a function");

            thisArg = @argument(2);
        }

        var usingSyncIterator;
        var usingAsyncIterator = asyncItems.@@asyncIterator;
        if (!@isUndefinedOrNull(usingAsyncIterator)) {
            if (!@isCallable(usingAsyncIterator))
                @throwTypeError("Array.fromAsync requires that the property of the first argument, items[Symbol.asyncIterator], when exists, be a function");
        } else {
            usingSyncIterator = asyncItems.@@iterator;
            if (!@isUndefinedOrNull(usingSyncIterator)) {
                if (!@isCallable(usingSyncIterator))
                    @throwTypeError("Array.fromAsync requires that the property of the first argument, items[Symbol.iterator], when exists, be a function");
            }
        }

        if (!@isUndefinedOrNull(usingAsyncIterator))
            return @defaultAsyncFromAsyncIterator.@call(this, usingAsyncIterator.@call(asyncItems), mapFn, thisArg);

        if (!@isUndefinedOrNull(usingSyncIterator)) {
            var iterator = usingSyncIterator.@call(asyncItems);
            return @defaultAsyncFromAsyncIterator.@call(this, @asyncFromSyncIteratorCreate(iterator), mapFn, thisArg);
        }

        return @defaultAsyncFromAsyncArrayLike.@call(this, asyncItems, mapFn, thisArg);
    } catch (reason) {
        var promise = @newPromise();
        @rejectPromiseWithFirstResolvingFunctionCallCheck(promise, reason);
        return promise;
    }
})
(function (iterable)
{
    "use strict";

    var object = {};

    for (var entry of iterable) {
        if (!@isObject(entry))
            @throwTypeError("Object.fromEntries requires the first iterable parameter yields objects");
        var key = entry[0];
        var value = entry[1];
        @putByValDirect(object, key, value);
    }

    return object;
})
(function (generator, state, value, resumeMode)
{
    "use strict";

    var done = true;
    if (state !== @GeneratorStateCompleted) {
        @putGeneratorInternalField(generator, @generatorFieldState, @GeneratorStateExecuting);

        try {
            var value = @getGeneratorInternalField(generator, @generatorFieldNext).@call(@getGeneratorInternalField(generator, @generatorFieldThis), generator, state, value, resumeMode, @getGeneratorInternalField(generator, @generatorFieldFrame));
        } catch (error) {
            @putGeneratorInternalField(generator, @generatorFieldState, @GeneratorStateCompleted);
            throw error;
        }

        done = @getGeneratorInternalField(generator, @generatorFieldState) === @GeneratorStateExecuting;
        if (done)
            @putGeneratorInternalField(generator, @generatorFieldState, @GeneratorStateCompleted);
    }
    return { value, done };
})
(function (target, propertyKey )
{
    "use strict";

    if (!@isObject(target))
        @throwTypeError("Reflect.get requires the first argument be an object");

    if (@argumentCount() < 3)
        return target[propertyKey];

    var receiver = @argument(2);
    return @getByValWithThis(target, receiver, propertyKey);
})
(function (value)
{
    'use strict';

    if (@isUndefinedOrNull(value))
        return @undefined;
    if (!@isObject(value))
        @throwTypeError("Disposable value must be an object, null, or undefined");

    var method = value.@@asyncDispose;
    if (@isUndefinedOrNull(method)) {
        method = value.@@dispose;
        if (@isUndefinedOrNull(method))
            @throwTypeError("@@asyncDispose and @@dispose must not be undefined or null");
        if (!@isCallable(method))
            @throwTypeError("@@dispose must be callable");
        var syncMethod = method;
        var receiver = value;
        return function () {
            try {
                syncMethod.@call(receiver);
            } catch (e) {
                return @promiseReject(@Promise, e);
            }
            return @promiseResolve(@Promise, @undefined);
        };
    }
    if (!@isCallable(method))
        @throwTypeError("@@asyncDispose must be callable");
    return method;
})
(function (value)
{
    'use strict';

    if (@isUndefinedOrNull(value))
        return @undefined;
    if (!@isObject(value))
        @throwTypeError("Disposable value must be an object, null, or undefined");
    var method = value.@@dispose;
    if (@isUndefinedOrNull(method))
        @throwTypeError("@@dispose must not be undefined or null");
    if (!@isCallable(method))
        @throwTypeError("@@dispose must be callable");
    return method;
})
(function (obj, rejectStrings)
{
    "use strict";

    //
    //
    if (!@isObject(obj) && (rejectStrings || typeof obj !== "string"))
        @throwTypeError("GetIteratorFlattenable expects its first argument to be an object" + (rejectStrings ? "" : " or a string"));
    //
    var method = obj.@@iterator;
    //
    //
    var iterator;
    if (@isUndefinedOrNull(method)) {
        iterator = obj;
        //
        //
    } else
        iterator = method.@call(obj);
    //
    if (!@isObject(iterator))
        @throwTypeError("Iterator is not an object");
    //
    return iterator;
})
(function (obj)
{
    "use strict";

    //
    //
    //
    var method = obj.@@iterator;

    //
    if (@isUndefinedOrNull(method))
        @throwTypeError("Symbol.iterator is not defined");
    if (!@isCallable(method))
        @throwTypeError("Symbol.iterator is not callable");

    //
    var iterator = method.@call(obj);
    if (!@isObject(iterator))
        @throwTypeError("Iterator is not an object");
    return iterator;
})
(function (options)
{
    //
    if (options === @undefined) {
        //
        return @Object.@create(null);
    }

    //
    if (@isObject(options)) {
        //
        return options;
    }

    //
    @throwTypeError("Expected object or null for options argument");
})
(function (items, callback)
{
    "use strict";

    if (@isUndefinedOrNull(items))
        @throwTypeError("Map.groupBy requires that the first argument not be null or undefined");

    if (!@isCallable(callback))
        @throwTypeError("Map.groupBy requires that the second argument must be a function");

    var iteratorMethod = items.@@iterator;
    if (!@isCallable(iteratorMethod))
        @throwTypeError("Map.groupBy requires that the property of the first argument, items[Symbol.iterator] be a function");

    var groups = new @Map;
    var k = 0;

    var iterator = iteratorMethod.@call(items);
    //
    //
    //
    var wrapper = {
        @@iterator: function () { return iterator; }
    };
    for (var item of wrapper) {
        if (k >= @MAX_SAFE_INTEGER)
            @throwTypeError("The number of iterations exceeds 2**53 - 1");
        var key = callback.@call(@undefined, item, k);
        var group = groups.@get(key);
        if (!group) {
            group = [];
            groups.@set(key, group);
        }
        @putByValDirect(group, group.length, item);
        ++k;
    }
    return groups;
})
(function (items, callback)
{
    "use strict";

    if (@isUndefinedOrNull(items))
        @throwTypeError("Object.groupBy requires that the first argument not be null or undefined");

    if (!@isCallable(callback))
        @throwTypeError("Object.groupBy requires that the second argument must be a function");

    var iteratorMethod = items.@@iterator;
    if (!@isCallable(iteratorMethod))
        @throwTypeError("Object.groupBy requires that the property of the first argument, items[Symbol.iterator] be a function");

    var groups = @Object.@create(null);
    var k = 0;

    var iterator = iteratorMethod.@call(items);
    //
    //
    //
    var wrapper = {
        @@iterator: function () { return iterator; }
    };
    for (var item of wrapper) {
        if (k >= @MAX_SAFE_INTEGER)
            @throwTypeError("The number of iterations exceeds 2**53 - 1");
        var key = @toPropertyKey(callback.@call(@undefined, item, k));
        var group = groups[key];
        if (!group) {
            group = [];
            @putByValDirect(groups, key, group);
        }
        @putByValDirect(group, group.length, item);
        ++k;
    }
    return groups;
})
(function (target, propertyKey)
{
    "use strict";

    if (!@isObject(target))
        @throwTypeError("Reflect.has requires the first argument be an object");

    return propertyKey in target;
})
(function (specifier, exportName)
{
    "use strict";

    if (!@isShadowRealm(this))
        @throwTypeError("`%ShadowRealm%.importValue requires that |this| be a ShadowRealm instance");

    var specifierString = @toString(specifier);

    if (typeof exportName !== 'string')
        @throwTypeError("`%ShadowRealm%.importValue requires that the |exportName| argument be a string");

    var lookupBinding = (module) => {
        var lookup = module[exportName]
        if (lookup === @undefined)
            @throwTypeError("%ShadowRealm%.importValue requires |exportName| to exist in the |specifier|");

        return @wrapRemoteValue(true, this, lookup);
    };

    return @importInRealm(this, specifierString).@then(lookupBinding, @crossRealmThrow);
})
(function (openIters)
{
    "use strict";

    var error;
    var hasError = false;
    for (var i = openIters.length - 1; i >= 0; --i) {
        try {
            @iteratorGenericClose(openIters[i]);
        } catch (e) {
            if (!hasError) {
                hasError = true;
                error = e;
            }
        }
    }
    if (hasError)
        throw error;
})
(function (iters, nextMethods, mode, padding, finishResults)
{
    "use strict";

    //
    var iterCount = iters.length;

    //
    var openIters = [];
    for (var i = 0; i < iterCount; ++i)
        @arrayPush(openIters, iters[i]);

    //
    //
    //
    //
    //
    //
    var underlyingIterators = @Object.@create(null);
    underlyingIterators.return = function () {
        @iteratorCloseAllNormal(openIters);
        return { done: true, value: @undefined };
    };

    //
    //
    //
    var generator = (function*() {
        //
        if (iterCount === 0)
            return;

        //
        for (;;) {
            //
            var results = [];

            //

            //
            for (var i = 0; i < iterCount; ++i) {
                //
                var iter = iters[i];
                var result;

                //
                if (iter === null) {
                    //
                    //
                    result = padding[i];
                } else {
                    //
                    var stepDone;
                    var stepValue;
                    try {
                        var stepResult = @iteratorGenericNext(nextMethods[i], iter);
                        stepDone = stepResult.done;
                        if (!stepDone)
                            stepValue = stepResult.value;
                    } catch (e) {
                        //
                        //
                        @removeFirstFromList(openIters, iter);
                        //
                        @closeAllIterators(openIters);
                        throw e;
                    }

                    //
                    //
                    if (stepDone) {
                        //
                        @removeFirstFromList(openIters, iter);

                        //
                        if (mode === "shortest") {
                            //
                            @iteratorCloseAllNormal(openIters);
                            return;
                        }

                        //
                        if (mode === "strict") {
                            //
                            if (i !== 0) {
                                //
                                @closeAllIterators(openIters);
                                @throwTypeError("Iterator.zip strict mode requires all iterables to have the same number of elements");
                            }

                            //
                            for (var k = 1; k < iterCount; ++k) {
                                //
                                var otherIter = iters[k];
                                var otherDone;
                                try {
                                    //
                                    var otherResult = @iteratorGenericNext(nextMethods[k], otherIter);
                                    otherDone = otherResult.done;
                                } catch (e) {
                                    //
                                    //
                                    @removeFirstFromList(openIters, otherIter);
                                    //
                                    @closeAllIterators(openIters);
                                    throw e;
                                }

                                //
                                if (otherDone) {
                                    //
                                    @removeFirstFromList(openIters, otherIter);
                                } else {
                                    //
                                    @closeAllIterators(openIters);
                                    @throwTypeError("Iterator.zip strict mode requires all iterables to have the same number of elements");
                                }
                            }

                            //
                            return;
                        }

                        //
                        //
                        if (openIters.length === 0)
                            return;
                        //
                        iters[i] = null;
                        //
                        result = padding[i];
                    } else {
                        result = stepValue;
                    }
                }

                //
                @arrayPush(results, result);
            }

            //
            results = finishResults(results);

            //
            //
            //
            @ifAbruptCloseIterator(underlyingIterators, (
                yield results
            ));
        }
    })();

    //
    //
    //
    return @iteratorHelperCreate(generator, underlyingIterators);
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.map requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.map callback must be a function");
    
    var thisArg = @argument(1);
    var result = @newArrayWithSpecies(length, array);

    for (var i = 0; i < length; i++) {
        if (!(i in array))
            continue;
        var mappedValue = callback.@call(thisArg, array[i], i, array);
        @putByValDirect(result, i, mappedValue);
    }
    return result;
})
(function (mapper)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.map requires that |this| be an Object.");

    if (!@isCallable(mapper)) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.map callback must be a function.");
        }
    }

    var iterated = this;
    var iteratedNextMethod = iterated.next;

    var generator = (function*() {
        var counter = 0;
        for (;;) {
            var result = @iteratorGenericNext(iteratedNextMethod, iterated);
            if (result.done)
                return;

            var value = result.value;
            @ifAbruptCloseIterator(iterated, (
                yield mapper(value, counter++)
            ));
        }
    })();

    return @iteratorHelperCreate(generator, iterated);
})
(function ()
{
    'use strict';

    if (!@isAsyncDisposableStack(this))
        @throwTypeError("AsyncDisposableStack.prototype.move requires that |this| be a AsyncDisposableStack object");

    if (@getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldState) === @AsyncDisposableStackStateDisposed)
        throw new @ReferenceError("AsyncDisposableStack.prototype.move requires that |this| be a pending AsyncDisposableStack object");

    var newAsyncDisposableStack = new @AsyncDisposableStack();
    @putAsyncDisposableStackInternalField(newAsyncDisposableStack, @asyncDisposableStackFieldCapability, @getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldCapability));

    @putAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldCapability, []);
    @putAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldState, @AsyncDisposableStackStateDisposed);

    return newAsyncDisposableStack;
})
(function ()
{
    'use strict';

    if (!@isDisposableStack(this))
        @throwTypeError("DisposableStack.prototype.move requires that |this| be a DisposableStack object");

    if (@getDisposableStackInternalField(this, @disposableStackFieldState) === @DisposableStackStateDisposed)
        throw new @ReferenceError("DisposableStack.prototype.move requires that |this| be a pending DisposableStack object");

    var newDisposableStack = new @DisposableStack();
    @putDisposableStackInternalField(newDisposableStack, @disposableStackFieldCapability, @getDisposableStackInternalField(this, @disposableStackFieldCapability));

    @putDisposableStackInternalField(this, @disposableStackFieldCapability, []);
    @putDisposableStackInternalField(this, @disposableStackFieldState, @DisposableStackStateDisposed);

    return newDisposableStack;
})
(function ()
{
    "use strict";

    if (!@isArrayIterator(this))
        @throwTypeError("%ArrayIteratorPrototype%.next requires that |this| be an Array Iterator instance");

    var index = @getArrayIteratorInternalField(this, @arrayIteratorFieldIndex);
    if (index === -1)
        return { value: @undefined, done: true };

    var array = @getArrayIteratorInternalField(this, @arrayIteratorFieldIteratedObject);
    if (@isTypedArrayView(array) && @isDetached(array))
        @throwTypeError("Underlying ArrayBuffer has been detached from the view or out-of-bounds");

    var kind = @getArrayIteratorInternalField(this, @arrayIteratorFieldKind);
    return @arrayIteratorNextHelper.@call(this, array, kind, index);
})
(function (value)
{
    "use strict";

    if (!@isGenerator(this))
        @throwTypeError("|this| should be a generator");

    var state = @getGeneratorInternalField(this, @generatorFieldState);
    if (state === @GeneratorStateExecuting)
        @throwTypeError("Generator is executing");

    if (state === @GeneratorStateCompleted)
        value = @undefined;

    return @generatorResume(this, state, value, @GeneratorResumeModeNormal);
})
(function ()
{
    "use strict";

    if (!@isIteratorHelper(this))
        @throwTypeError("|this| should be an iterator helper");

    var generator = @getIteratorHelperInternalField(this, @iteratorHelperFieldGenerator);
    var state = @getGeneratorInternalField(generator, @generatorFieldState);
    if (state === @GeneratorStateExecuting)
        @throwTypeError("Generator is executing");

    return @generatorResume(generator, state, @undefined, @GeneratorResumeModeNormal);
})
(function ()
{
    "use strict";

    //
    //
    //
    if (!@isWrapForValidIterator(this))
        @throwTypeError("%WrapForValidIteratorPrototype%.next requires that |this| be a WrapForValidIteratorPrototype object");

    //
    return @getWrapForValidIteratorInternalField(this, @wrapForValidIteratorFieldIteratedNextMethod).@call(@getWrapForValidIteratorInternalField(this, @wrapForValidIteratorFieldIteratedIterator));
})
(function (iterable)
{
    "use strict";
    //
    //
    //

    var result = [];
    if (@isUndefinedOrNull(iterable))
        @throwTypeError('Spread syntax requires ...iterable not be null or undefined');
    
    var iteratorMethod = iterable.@@iterator;
    if (!@isCallable(iteratorMethod))
        @throwTypeError('Spread syntax requires ...iterable[Symbol.iterator] to be a function');

    var iterator = iteratorMethod.@call(iterable);
    var next = iterator.next;
    var item;
    var index = 0;
    while (true) {
        item = next.@call(iterator);
        if (!@isObject(item))
            @throwTypeError("Iterator result interface is not an object");
        if (item.done)
            return result;
        @putByValDirect(result, index++, item.value);
    }
})
(function (propertyName, receiver)
{
    "use strict";

    var target = @getProxyInternalField(this, @proxyFieldTarget);
    var handler = @getProxyInternalField(this, @proxyFieldHandler);

    if (handler === null)
        @throwTypeError("Proxy has already been revoked. No more operations are allowed to be performed on it");

    var trap = handler.get;
    if (@isUndefinedOrNull(trap))
        return @getByValWithThis(target, receiver, propertyName);

    if (!@isCallable(trap))
        @throwTypeError("'get' property of a Proxy's handler should be callable");

    var trapResult = trap.@call(handler, target, propertyName, @toThis(receiver));

    if (@mustValidateResultOfProxyGetAndSetTraps(target))
        @handleProxyGetTrapResult(trapResult, target, propertyName);

    return trapResult;
})
(function (propertyName, receiver)
{
    "use strict";

    var target = @getProxyInternalField(this, @proxyFieldTarget);
    var handler = @getProxyInternalField(this, @proxyFieldHandler);
    var propertyName = @toPropertyKey(propertyName);

    if (handler === null)
        @throwTypeError("Proxy has already been revoked. No more operations are allowed to be performed on it");

    var trap = handler.get;
    if (@isUndefinedOrNull(trap))
        return @getByValWithThis(target, receiver, propertyName);

    if (!@isCallable(trap))
        @throwTypeError("'get' property of a Proxy's handler should be callable");

    var trapResult = trap.@call(handler, target, propertyName, @toThis(receiver));

    if (@mustValidateResultOfProxyGetAndSetTraps(target))
        @handleProxyGetTrapResult(trapResult, target, propertyName);

    return trapResult;
})
(function (propertyName)
{
    "use strict";

    var target = @getProxyInternalField(this, @proxyFieldTarget);
    var handler = @getProxyInternalField(this, @proxyFieldHandler);

    if (handler === null)
        @throwTypeError("Proxy has already been revoked. No more operations are allowed to be performed on it");

    var trap = handler.has;
    if (@isUndefinedOrNull(trap))
        return propertyName in target;

    if (!@isCallable(trap))
        @throwTypeError("'has' property of a Proxy's handler should be callable");

    if (trap.@call(handler, target, propertyName))
        return true;

    if (@mustValidateResultOfProxyTrapsExceptGetAndSet(target))
        @handleNegativeProxyHasTrapResult(target, propertyName);

    return false;
})
(function (propertyName)
{
    "use strict";

    var target = @getProxyInternalField(this, @proxyFieldTarget);
    var handler = @getProxyInternalField(this, @proxyFieldHandler);
    var propertyName = @toPropertyKey(propertyName);

    if (handler === null)
        @throwTypeError("Proxy has already been revoked. No more operations are allowed to be performed on it");

    var trap = handler.has;
    if (@isUndefinedOrNull(trap))
        return propertyName in target;

    if (!@isCallable(trap))
        @throwTypeError("'has' property of a Proxy's handler should be callable");

    if (trap.@call(handler, target, propertyName))
        return true;

    if (@mustValidateResultOfProxyTrapsExceptGetAndSet(target))
        @handleNegativeProxyHasTrapResult(target, propertyName);

    return false;
})
(function (propertyName, receiver, value)
{
    "use strict";

    var target = @getProxyInternalField(this, @proxyFieldTarget);
    var handler = @getProxyInternalField(this, @proxyFieldHandler);
    var propertyName = @toPropertyKey(propertyName);

    if (handler === null)
        @throwTypeError("Proxy has already been revoked. No more operations are allowed to be performed on it");

    var trap = handler.set;
    if (@isUndefinedOrNull(trap)) {
        @putByValWithThisSloppy(target, receiver, propertyName, value);
        return;
    }

    if (!@isCallable(trap))
        @throwTypeError("'set' property of a Proxy's handler should be callable");

    if (!trap.@call(handler, target, propertyName, value, @toThis(receiver)))
        return;

    if (@mustValidateResultOfProxyGetAndSetTraps(target))
        @handlePositiveProxySetTrapResult(target, propertyName, value);
})
(function (propertyName, receiver, value)
{
    "use strict";

    var target = @getProxyInternalField(this, @proxyFieldTarget);
    var handler = @getProxyInternalField(this, @proxyFieldHandler);
    var propertyName = @toPropertyKey(propertyName);

    if (handler === null)
        @throwTypeError("Proxy has already been revoked. No more operations are allowed to be performed on it");

    var trap = handler.set;
    if (@isUndefinedOrNull(trap)) {
        @putByValWithThisStrict(target, receiver, propertyName, value);
        return;
    }

    if (!@isCallable(trap))
        @throwTypeError("'set' property of a Proxy's handler should be callable");

    if (!trap.@call(handler, target, propertyName, value, @toThis(receiver)))
        @throwTypeError("Proxy object's 'set' trap returned falsy value for property '" + @String(propertyName) + "'");

    if (@mustValidateResultOfProxyGetAndSetTraps(target))
        @handlePositiveProxySetTrapResult(target, propertyName, value);
})
(function (propertyName, receiver, value)
{
    "use strict";

    var target = @getProxyInternalField(this, @proxyFieldTarget);
    var handler = @getProxyInternalField(this, @proxyFieldHandler);

    if (handler === null)
        @throwTypeError("Proxy has already been revoked. No more operations are allowed to be performed on it");

    var trap = handler.set;
    if (@isUndefinedOrNull(trap)) {
        @putByValWithThisSloppy(target, receiver, propertyName, value);
        return;
    }

    if (!@isCallable(trap))
        @throwTypeError("'set' property of a Proxy's handler should be callable");

    if (!trap.@call(handler, target, propertyName, value, @toThis(receiver)))
        return;

    if (@mustValidateResultOfProxyGetAndSetTraps(target))
        @handlePositiveProxySetTrapResult(target, propertyName, value);
})
(function (propertyName, receiver, value)
{
    "use strict";

    var target = @getProxyInternalField(this, @proxyFieldTarget);
    var handler = @getProxyInternalField(this, @proxyFieldHandler);

    if (handler === null)
        @throwTypeError("Proxy has already been revoked. No more operations are allowed to be performed on it");

    var trap = handler.set;
    if (@isUndefinedOrNull(trap)) {
        @putByValWithThisStrict(target, receiver, propertyName, value);
        return;
    }

    if (!@isCallable(trap))
        @throwTypeError("'set' property of a Proxy's handler should be callable");

    if (!trap.@call(handler, target, propertyName, value, @toThis(receiver)))
        @throwTypeError("Proxy object's 'set' trap returned falsy value for property '" + @String(propertyName) + "'");

    if (@mustValidateResultOfProxyGetAndSetTraps(target))
        @handlePositiveProxySetTrapResult(target, propertyName, value);
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.reduce requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.reduce callback must be a function");

    var argumentCount = @argumentCount();
    if (length === 0 && argumentCount < 2)
        @throwTypeError("reduce of empty array with no initial value");

    var accumulator, k = 0;
    if (argumentCount > 1)
        accumulator = @argument(1);
    else {
        while (k < length && !(k in array))
            k += 1;
        if (k >= length)
            @throwTypeError("reduce of empty array with no initial value");
        accumulator = array[k++];
    }

    while (k < length) {
        if (k in array)
            accumulator = callback.@call(@undefined, accumulator, array[k], k, array);
        k += 1;
    }
    return accumulator;
})
(function (reducer )
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.reduce requires that |this| be an Object.");

    if (!@isCallable(reducer)) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.reduce reducer argument must be a function.");
        }
    }

    var iterated = this;
    var iteratedNextMethod = this.next;

    var accumulator;
    var counter = 0;
    if (@argumentCount() <= 1) {
        var result = @iteratorGenericNext(iteratedNextMethod, iterated);
        if (result.done)
            @throwTypeError("Iterator.prototype.reduce requires an initial value or an iterator that is not done.");
        accumulator = result.value;
        counter = 1;
    } else
        accumulator = @argument(1);

    for (;;) {
        var result = @iteratorGenericNext(iteratedNextMethod, iterated);
        if (result.done)
            break;

        var value = result.value;
        @ifAbruptCloseIterator(iterated, (
            accumulator = reducer(accumulator, value, counter++)
        ));
    }

    return accumulator;
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.reduceRight requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.reduceRight callback must be a function");

    var argumentCount = @argumentCount();
    if (length === 0 && argumentCount < 2)
        @throwTypeError("reduceRight of empty array with no initial value");

    var accumulator, k = length - 1;
    if (argumentCount > 1)
        accumulator = @argument(1);
    else {
        while (k >= 0 && !(k in array))
            k -= 1;
        if (k < 0)
            @throwTypeError("reduceRight of empty array with no initial value");
        accumulator = array[k--];
    }

    while (k >= 0) {
        if (k in array)
            accumulator = callback.@call(@undefined, accumulator, array[k], k, array);
        k -= 1;
    }
    return accumulator;
})
(function (list, value)
{
    "use strict";

    var length = list.length;
    var shifting = false;
    for (var i = 0; i < length; ++i) {
        if (shifting)
            @putByValDirect(list, i - 1, list[i]);
        else if (list[i] === value)
            shifting = true;
    }
    if (shifting)
        list.length = length - 1;
})
(function (value)
{
    "use strict";

    if (!@isGenerator(this))
        @throwTypeError("|this| should be a generator");

    var state = @getGeneratorInternalField(this, @generatorFieldState);
    if (state === @GeneratorStateExecuting)
        @throwTypeError("Generator is executing");

    return @generatorResume(this, state, value, @GeneratorResumeModeReturn);
})
(function ()
{
    "use strict";

    if (!@isIteratorHelper(this))
        @throwTypeError("|this| should be an iterator helper");

    var generator = @getIteratorHelperInternalField(this, @iteratorHelperFieldGenerator);
    var state = @getGeneratorInternalField(generator, @generatorFieldState);
    if (state === @GeneratorStateInit) {
        @putGeneratorInternalField(generator, @generatorFieldState, @GeneratorStateCompleted);

        var underlyingIterator = @getIteratorHelperInternalField(this, @iteratorHelperFieldUnderlyingIterator);
        if (underlyingIterator !== null)
            @iteratorGenericClose(underlyingIterator);

        return { value: @undefined, done: true };
    }

    if (state === @GeneratorStateExecuting)
        @throwTypeError("Generator is executing");

    return @generatorResume(generator, state, @undefined, @GeneratorResumeModeReturn);
})
(function ()
{
    "use strict";

    //
    //
    //
    if (!@isWrapForValidIterator(this))
        @throwTypeError("%WrapForValidIteratorPrototype%.next requires that |this| be a WrapForValidIteratorPrototype object");

    //
    var iterator = @getWrapForValidIteratorInternalField(this, @wrapForValidIteratorFieldIteratedIterator);
    //
    @assert(@isObject(iterator));
    //
    var returnMethod = iterator.return;
    //
    //
    if (@isUndefinedOrNull(returnMethod))
        return { value: @undefined, done: true };
    //
    return returnMethod.@call(iterator);
})
(function (callback )
{
    "use strict";

    var array = @toObject(this, "Array.prototype.some requires that |this| not be null or undefined");
    var length = @toLength(array.length);

    if (!@isCallable(callback))
        @throwTypeError("Array.prototype.some callback must be a function");
    
    var thisArg = @argument(1);
    for (var i = 0; i < length; i++) {
        if (!(i in array))
            continue;
        if (callback.@call(thisArg, array[i], i, array))
            return true;
    }
    return false;
})
(function (predicate)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.some requires that |this| be an Object.");

    if (!@isCallable(predicate)) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.some callback must be a function.");
        }
    }

    var iterated = this;
    var count = 0;
    var wrapper = { @@iterator: function () { return iterated; }};
    for (var item of wrapper) {
        if (predicate(item, count++))
            return true;
    }

    return false;
})
(function (limit)
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.take requires that |this| be an Object.");

    var numLimit;
    @ifAbruptCloseIterator(this, numLimit = @toNumber(limit));
    if (numLimit !== numLimit) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwRangeError("Iterator.prototype.take argument must not be NaN.");
        }
    }

    if (@isFinite(numLimit) && numLimit > @MAX_SAFE_INTEGER) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwRangeError("Iterator.prototype.take argument must be non-negative safe integer or +Infinity");
        }
    }

    var intLimit = @toIntegerOrInfinity(numLimit);
    if (intLimit < 0) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwRangeError("Iterator.prototype.take argument must be non-negative safe integer or +Infinity");
        }
    }

    var iterated = this;
    var iteratedNextMethod = iterated.next;

    var generator = (function*() {
        var remaining = intLimit;
        for (;;) {
            if (remaining === 0) {
                @iteratorGenericClose(iterated);
                return;
            }

            if (remaining !== @Infinity)
                remaining--;

            var result = @iteratorGenericNext(iteratedNextMethod, iterated);
            if (result.done)
                return;

            var value = result.value;
            @ifAbruptCloseIterator(iterated, (
                yield value
            ));
        }
    })();

    return @iteratorHelperCreate(generator, iterated);
})
(function (exception)
{
    "use strict";

    if (!@isGenerator(this))
        @throwTypeError("|this| should be a generator");

    var state = @getGeneratorInternalField(this, @generatorFieldState);
    if (state === @GeneratorStateExecuting)
        @throwTypeError("Generator is executing");

    if (state === @GeneratorStateCompleted)
        throw exception;

    return @generatorResume(this, state, exception, @GeneratorResumeModeThrow);
})
(function ()
{
    "use strict";

    var length = @typedArrayLength(this);

    if (length == 0)
        return "";

    var string = "";
    for (var i = 0; i < length; ++i) {
        if (i > 0)
            string += ",";
        var element = this[i];
        if (!@isUndefinedOrNull(element))
            string += @toString(element.toLocaleString(@argument(0), @argument(1)));
    }

    return string;
})
(function (callback )
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("|this| is not an object");

    var args = [];
    for (var i = 1; i < @argumentCount(); i++)
        @putByValDirect(args, i - 1, arguments[i]);

    try {
        var value = callback.@apply(@undefined, args);
    } catch (error) {
        return @promiseReject(this, error);
    }
    return @promiseResolve(this, value);
})
(function (value)
{
    'use strict';

    if (!@isAsyncDisposableStack(this))
        @throwTypeError("AsyncDisposableStack.prototype.use requires that |this| be a AsyncDisposableStack object");

    if (@getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldState) === @AsyncDisposableStackStateDisposed)
        throw new @ReferenceError("AsyncDisposableStack.prototype.use requires that |this| be a pending AsyncDisposableStack object");

    @addDisposableResource(@getAsyncDisposableStackInternalField(this, @asyncDisposableStackFieldCapability), value,  true);

    return value;
})
(function (value)
{
    'use strict';

    if (!@isDisposableStack(this))
        @throwTypeError("DisposableStack.prototype.use requires that |this| be a DisposableStack object");

    if (@getDisposableStackInternalField(this, @disposableStackFieldState) === @DisposableStackStateDisposed)
        throw new @ReferenceError("DisposableStack.prototype.use requires that |this| be a pending DisposableStack object");

    @addDisposableResource(@getDisposableStackInternalField(this, @disposableStackFieldCapability), value,  false);

    return value;
})
(function (windowSize )
{
    "use strict";

    if (!@isObject(this))
        @throwTypeError("Iterator.prototype.windows requires that |this| be an Object.");

    if (typeof windowSize !== "number" || windowSize % 1 !== 0) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.windows requires that first argument be an integral Number.");
        }
    }

    if (windowSize < 1 || windowSize > 4294967295) {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwRangeError("Iterator.prototype.windows requires that first argument be between 1 and 2**32 - 1.");
        }
    }

    var undersized = @argument(1);
    if (undersized === @undefined)
        undersized = "only-full";
    else if (undersized !== "only-full" && undersized !== "allow-partial") {
        try {
            @iteratorGenericClose(this);
        } finally {
            @throwTypeError("Iterator.prototype.windows requires that second argument be \"only-full\" or \"allow-partial\".");
        }
    }

    var iterated = this;
    var iteratedNextMethod = this.next;

    var generator = (function*() {
        var buffer = [];

        for (;;) {
            var result = @iteratorGenericNext(iteratedNextMethod, iterated);
            if (result.done) {
                if (undersized === "allow-partial" && buffer.length && buffer.length < windowSize)
                    yield buffer;
                return;
            }

            if (buffer.length === windowSize) {
                for (var i = 0; i < buffer.length - 1; ++i)
                    buffer[i] = buffer[i + 1];
                buffer[buffer.length - 1] = result.value;
            } else
                @arrayPush(buffer, result.value);

            if (buffer.length === windowSize) {
                var copy = @newArrayWithSize(buffer.length);
                for (var i = 0; i < buffer.length; ++i)
                    copy[i] = buffer[i];

                @ifAbruptCloseIterator(iterated, (
                    yield copy
                ));
            }
        }
    })();

    return @iteratorHelperCreate(generator, iterated);
})
(function (fromShadowRealm, shadowRealm, target)
{
    "use strict";

    if (@isCallable(target))
        return @createRemoteFunction(target, fromShadowRealm ? null : shadowRealm);

    if (@isObject(target))
        @throwTypeError("value passing between realms must be callable or primitive");

    return target;
})
(function (iterator)
{
    var wrapper = @Object.@create(null);
    wrapper.@@iterator = function() { return iterator; }
    return wrapper;
})
(function (iterables )
{
    "use strict";

    var options = @argument(1);

    //
    if (!@isObject(iterables))
        @throwTypeError("Iterator.zip expects its first argument to be an object");

    //
    options = @getOptionsObject(options);

    //
    var mode = options.mode;

    //
    if (mode === @undefined)
        mode = "shortest";

    //
    if (mode !== "shortest" && mode !== "longest" && mode !== "strict")
        @throwTypeError("Expected \"shortest\", \"longest\" or \"strict\" for options.mode");

    //
    var paddingOption = @undefined;

    //
    if (mode === "longest") {
        //
        paddingOption = options.padding;
        //
        if (paddingOption !== @undefined && !@isObject(paddingOption))
            @throwTypeError("Expected options.padding to be an object");
    }

    //
    var iters = [];
    var nextMethods = [];

    //
    var padding = [];

    //
    var inputIter = @getIteratorSync(iterables);
    var inputIterNextMethod = inputIter.next;

    //
    //
    for (;;) {
        var done;
        var value;
        try {
            //
            var result = @iteratorGenericNext(inputIterNextMethod, inputIter);
            done = result.done;
            if (!done)
                value = result.value;
        } catch (e) {
            //
            @closeAllIterators(iters);
            throw e;
        }

        //
        if (done)
            break;

        var iter;
        var iterNextMethod;
        try {
            //
            iter = @getIteratorFlattenable(value,  true);
            iterNextMethod = iter.next;
        } catch (e) {
            //
            @closeAllIterators(iters);
            try {
                @iteratorGenericClose(inputIter);
            } catch (e2) { }
            throw e;
        }

        //
        @arrayPush(iters, iter);
        @arrayPush(nextMethods, iterNextMethod);
    }

    //
    var iterCount = iters.length;

    //
    if (mode === "longest") {
        //
        if (paddingOption === @undefined) {
            //
            for (var i = 0; i < iterCount; ++i) {
                //
                @arrayPush(padding, @undefined);
            }
        } else {
            //
            var paddingIter;
            var paddingIterNextMethod;
            try {
                paddingIter = @getIteratorSync(paddingOption);
                paddingIterNextMethod = paddingIter.next;
            } catch (e) {
                //
                @closeAllIterators(iters);
                throw e;
            }

            //
            var usingIterator = true;

            //
            for (var i = 0; i < iterCount; ++i) {
                //
                if (usingIterator) {
                    var paddingDone;
                    var paddingValue;
                    try {
                        //
                        var paddingResult = @iteratorGenericNext(paddingIterNextMethod, paddingIter);
                        paddingDone = paddingResult.done;
                        if (!paddingDone)
                            paddingValue = paddingResult.value;
                    } catch (e) {
                        //
                        @closeAllIterators(iters);
                        throw e;
                    }

                    //
                    if (paddingDone) {
                        //
                        usingIterator = false;
                    } else {
                        //
                        @arrayPush(padding, paddingValue);
                    }
                }

                //
                if (!usingIterator)
                    @arrayPush(padding, @undefined);
            }

            //
            if (usingIterator) {
                try {
                    //
                    @iteratorGenericClose(paddingIter);
                } catch (e) {
                    //
                    @closeAllIterators(iters);
                    throw e;
                }
            }
        }
    }

    //
    var finishResults = (results) => {
        //
        return results;
    };

    //
    return @iteratorZip(iters, nextMethods, mode, padding, finishResults);
})
(function (iterables )
{
    "use strict";

    var options = @argument(1);

    //
    if (!@isObject(iterables))
        @throwTypeError("Iterator.zipKeyed expects its first argument to be an object");

    //
    options = @getOptionsObject(options);

    //
    var mode = options.mode;

    //
    if (mode === @undefined)
        mode = "shortest";

    //
    if (mode !== "shortest" && mode !== "longest" && mode !== "strict")
        @throwTypeError("Expected \"shortest\", \"longest\" or \"strict\" for options.mode");

    //
    var paddingOption = @undefined;

    //
    if (mode === "longest") {
        //
        paddingOption = options.padding;
        //
        if (paddingOption !== @undefined && !@isObject(paddingOption))
            @throwTypeError("Expected options.padding to be an object");
    }

    //
    var iters = [];
    var nextMethods = [];

    //
    var padding = [];

    //
    var allKeys = @ownKeys(iterables);

    //
    var keys = [];

    //
    var allKeysCount = allKeys.length;
    for (var k = 0; k < allKeysCount; ++k) {
        var key = allKeys[k];

        //
        var desc;
        try {
            desc = @Object.@getOwnPropertyDescriptor(iterables, key);
        } catch (e) {
            //
            @closeAllIterators(iters);
            throw e;
        }

        //
        if (desc !== @undefined && desc.enumerable) {
            //
            var value;
            try {
                value = iterables[key];
            } catch (e) {
                //
                @closeAllIterators(iters);
                throw e;
            }

            //
            if (value !== @undefined) {
                //
                @arrayPush(keys, key);

                //
                var iter;
                var iterNextMethod;
                try {
                    iter = @getIteratorFlattenable(value,  true);
                    iterNextMethod = iter.next;
                } catch (e) {
                    //
                    @closeAllIterators(iters);
                    throw e;
                }

                //
                @arrayPush(iters, iter);
                @arrayPush(nextMethods, iterNextMethod);
            }
        }
    }

    //
    var iterCount = iters.length;

    //
    if (mode === "longest") {
        //
        if (paddingOption === @undefined) {
            //
            for (var i = 0; i < iterCount; ++i) {
                //
                @arrayPush(padding, @undefined);
            }
        } else {
            //
            for (var i = 0; i < iterCount; ++i) {
                //
                var paddingValue;
                try {
                    paddingValue = paddingOption[keys[i]];
                } catch (e) {
                    //
                    @closeAllIterators(iters);
                    throw e;
                }
                //
                @arrayPush(padding, paddingValue);
            }
        }
    }

    //
    var finishResults = (results) => {
        //
        var obj = @Object.@create(null);
        //
        for (var i = 0; i < iterCount; ++i) {
            //
            @putByValDirect(obj, keys[i], results[i]);
        }
        //
        return obj;
    };

    //
    return @iteratorZip(iters, nextMethods, mode, padding, finishResults);
})
