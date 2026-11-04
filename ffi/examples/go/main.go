// go run . — Fogu embedded in a Go program (see fogu/fogu.go for the build flags).
package main

import (
	"encoding/json"
	"fmt"
	"log"
	"strings"
	"time"

	"example.com/fogu-embed/fogu"
)

func main() {
	rt, err := fogu.New()
	if err != nil {
		log.Fatal(err)
	}
	defer rt.Close()
	fmt.Println("fogu", fogu.Version())

	// 1. evaluate code and call Fogu functions with JSON values
	if _, err := rt.Eval(`
fn greet(name, n) {
    return { msg: "hi " + name, twice: n * 2 }
}
fn primes(limit) {
    const out = []
    for n in 2..limit {
        let prime = true
        for d in 2..n {
            if d * d > n { break }
            if n % d == 0 { prime = false; break }
        }
        if prime { out.push(n) }
    }
    return out
}`); err != nil {
		log.Fatal(err)
	}
	v, _ := rt.Call("greet", "go", 21)
	fmt.Println(string(v)) // {"msg":"hi go","twice":42}
	var ps []int
	v, _ = rt.Call("primes", 30)
	_ = json.Unmarshal(v, &ps)
	fmt.Println(ps) // [2 3 5 7 11 13 17 19 23 29]

	// 2. globals both ways
	_ = rt.Set("config", map[string]any{"retries": 3, "hosts": []string{"a", "b"}})
	v, _ = rt.Eval("len(config.hosts) * config.retries")
	fmt.Println(string(v)) // 6

	// 3. events: Go → Fogu (Fogu.signal.on) and Fogu → Go (Fogu.host.emit)
	done := make(chan json.RawMessage, 1)
	_ = rt.On("done", func(d json.RawMessage) { done <- d })
	_, _ = rt.Eval(`Fogu.signal.on("job", fn(d) { Fogu.host.emit("done", { id: d.id, square: d.id * d.id }) })`)
	_ = rt.Emit("job", map[string]int{"id": 7})
	select {
	case d := <-done:
		fmt.Println("done:", string(d)) // done: {"id":7,"square":49}
	case <-time.After(2 * time.Second):
		fmt.Println("no event")
	}

	// 4. Go functions the program can call (fogu_register)
	_ = rt.Register("goUpper", func(args []json.RawMessage) (any, error) {
		var s string
		if len(args) != 1 || json.Unmarshal(args[0], &s) != nil {
			return nil, fmt.Errorf("goUpper(text) expects a text")
		}
		return strings.ToUpper(s), nil
	})
	v, _ = rt.Eval(`const [bad, err] = goUpper(42)
[goUpper("fogu"), err.message]`)
	fmt.Println(string(v)) // ["FOGU","goUpper(text) expects a text"]

	// 5. Fogu errors come back as Go errors
	if _, err := rt.Eval("1 / 0"); err != nil {
		fmt.Println("error:", err)
	}
}
