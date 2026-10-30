# Design note: `fogu:events`

Source: [`src/native/events.rs`](../../src/native/events.rs).

## API

A Node.js-style `EventEmitter`:

```fogu
import events from "fogu:events"

const bus = events.EventEmitter()
bus.on("request", (msg) => print("got:", msg))
bus.once("startup", () => print("runs only once"))

bus.emit("request", "hello")     // returns true if a listener ran
bus.emit("startup")
bus.emit("startup")              // no longer fires

print(bus.listeners("request"), bus.eventNames(), bus.hasListeners("request"))
bus.off("request")               // removes the listeners of "request"
bus.removeAll()                  // removes everything
```

## How `emit` calls Fogu functions

`emit` must invoke Fogu functions, but a native method has no access to the
interpreter. Instead of giving native objects a handle to the interpreter, the
interpreter does the work: `Interpreter::call_value` **intercepts**
`emit` on an `EventEmitter` object, reads a snapshot of the listeners
(`with_emitter(|e| e.get_listeners(event))`), calls each one, and removes the
`once` listeners by id after they run. Taking a snapshot first means listeners
may add or remove listeners while the event is being emitted.

To reach the concrete emitter behind a `dyn FoguObject`, the object trait has an
`as_any` downcast, and `NativeObjectRef` offers `with_emitter` /
`with_emitter_mut`. The `emit` method of the native object itself only returns
an error ("must be handled by the interpreter"), so it is never reached by
accident.

## Notes

- Listeners run synchronously, in registration order.
- `emit` with no listeners returns `false`.
