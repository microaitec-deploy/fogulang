// Package fogu embeds the Fogu runtime (libfogu_rt) in a Go program through cgo.
// Values cross as JSON.
//
// Build: put fogu.h and the library of your platform in a folder and point cgo at it:
//
//	CGO_CFLAGS="-I$PWD/lib" CGO_LDFLAGS="-L$PWD/lib -lfogu_rt" go build
//
// (on Linux also run with LD_LIBRARY_PATH=$PWD/lib, or install the .so system-wide;
// on Windows keep fogu_rt.dll next to the .exe).
package fogu

/*
#include <stdint.h>
#include <stdlib.h>
#include "fogu.h"

extern void foguGoHost(char *name, char *json, void *user);

// the cgo.Handle travels as an integer: C must not keep Go pointers
static char *fogu_on_host_go(FoguRuntime *rt, uintptr_t handle) {
	return fogu_on_host(rt, (fogu_host_fn)foguGoHost, (void *)handle);
}
*/
import "C"

import (
	"encoding/json"
	"errors"
	"runtime/cgo"
	"sync"
	"unsafe"
)

// Runtime is a Fogu runtime: its globals, fg tasks and servers live until Close.
type Runtime struct {
	rt       *C.FoguRuntime
	mu       sync.Mutex
	handlers map[string][]func(json.RawMessage)
	handle   cgo.Handle
}

// Version of the Fogu runtime.
func Version() string { return C.GoString(C.fogu_version()) }

// New starts a runtime.
func New() (*Runtime, error) {
	rt := C.fogu_new()
	if rt == nil {
		return nil, errors.New("fogu_new failed")
	}
	return &Runtime{rt: rt, handlers: map[string][]func(json.RawMessage){}}, nil
}

type answer struct {
	OK    bool            `json:"ok"`
	Value json.RawMessage `json:"value"`
	Error string          `json:"error"`
}

// take reads a {"ok","value"|"error"} answer and frees it.
func take(p *C.char) (json.RawMessage, error) {
	if p == nil {
		return nil, errors.New("fogu: null answer")
	}
	defer C.fogu_free_string(p)
	var a answer
	if err := json.Unmarshal([]byte(C.GoString(p)), &a); err != nil {
		return nil, err
	}
	if !a.OK {
		return nil, errors.New(a.Error)
	}
	return a.Value, nil
}

func cstr(s string) *C.char { return C.CString(s) }

// Eval runs code in the global scope and returns the value of its last expression.
func (r *Runtime) Eval(code string) (json.RawMessage, error) {
	c := cstr(code)
	defer C.free(unsafe.Pointer(c))
	return take(C.fogu_eval(r.rt, c))
}

// Spawn runs code as a background task (servers, loops) and returns at once.
func (r *Runtime) Spawn(code string) error {
	c := cstr(code)
	defer C.free(unsafe.Pointer(c))
	_, err := take(C.fogu_spawn(r.rt, c))
	return err
}

// RunFile runs a .fg file.
func (r *Runtime) RunFile(path string) (json.RawMessage, error) {
	c := cstr(path)
	defer C.free(unsafe.Pointer(c))
	return take(C.fogu_run_file(r.rt, c))
}

// Call calls a global Fogu function; the arguments are encoded as JSON.
func (r *Runtime) Call(name string, args ...any) (json.RawMessage, error) {
	if args == nil {
		args = []any{}
	}
	js, err := json.Marshal(args)
	if err != nil {
		return nil, err
	}
	n, a := cstr(name), cstr(string(js))
	defer C.free(unsafe.Pointer(n))
	defer C.free(unsafe.Pointer(a))
	return take(C.fogu_call(r.rt, n, a))
}

// Get reads a global variable.
func (r *Runtime) Get(name string) (json.RawMessage, error) {
	n := cstr(name)
	defer C.free(unsafe.Pointer(n))
	return take(C.fogu_get(r.rt, n))
}

// Set defines or replaces a global variable.
func (r *Runtime) Set(name string, value any) error {
	js, err := json.Marshal(value)
	if err != nil {
		return err
	}
	n, v := cstr(name), cstr(string(js))
	defer C.free(unsafe.Pointer(n))
	defer C.free(unsafe.Pointer(v))
	_, err = take(C.fogu_set(r.rt, n, v))
	return err
}

// Emit fires the program's Fogu.signal.on(name, fn) handlers.
func (r *Runtime) Emit(name string, data any) error {
	js, err := json.Marshal(data)
	if err != nil {
		return err
	}
	n, d := cstr(name), cstr(string(js))
	defer C.free(unsafe.Pointer(n))
	defer C.free(unsafe.Pointer(d))
	_, err = take(C.fogu_emit(r.rt, n, d))
	return err
}

// On receives Fogu.host.emit(name, data) from the program ("*" receives every
// event). Handlers run on the runtime's thread: keep them short or hand the
// data to a goroutine.
func (r *Runtime) On(name string, fn func(json.RawMessage)) error {
	r.mu.Lock()
	first := r.handle == 0
	if first {
		r.handle = cgo.NewHandle(r)
	}
	r.handlers[name] = append(r.handlers[name], fn)
	r.mu.Unlock()
	if first {
		_, err := take(C.fogu_on_host_go(r.rt, C.uintptr_t(r.handle)))
		return err
	}
	return nil
}

//export foguGoHost
func foguGoHost(name *C.char, js *C.char, user unsafe.Pointer) {
	event, data := C.GoString(name), json.RawMessage(C.GoString(js))
	C.fogu_free_string(name)
	C.fogu_free_string(js)
	r := cgo.Handle(uintptr(user)).Value().(*Runtime)
	r.mu.Lock()
	hs := append(append([]func(json.RawMessage){}, r.handlers[event]...), r.handlers["*"]...)
	r.mu.Unlock()
	for _, h := range hs {
		h(data)
	}
}

// Close stops the program's tasks and servers and frees the runtime.
func (r *Runtime) Close() {
	if r.rt != nil {
		C.fogu_free(r.rt)
		r.rt = nil
	}
	if r.handle != 0 {
		r.handle.Delete()
		r.handle = 0
	}
}
