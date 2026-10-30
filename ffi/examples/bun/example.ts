// bun example.ts — Fogu embedded in a Bun program.
import { Fogu } from "./fogu.ts";

const fogu = new Fogu();
console.log("fogu", fogu.version);

// 1. evaluate code and call Fogu functions with JSON values
fogu.eval(`
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
}
`);
console.log(fogu.call("greet", "bun", 21)); // { msg: "hi bun", twice: 42 }
console.log(fogu.call("primes", 30)); // [ 2, 3, 5, 7, 11, 13, 17, 19, 23, 29 ]

// 2. globals both ways
fogu.set("config", { retries: 3, hosts: ["a", "b"] });
console.log(fogu.eval("len(config.hosts) * config.retries")); // 6

// 3. events: Bun → Fogu (Fogu.signal.on) and Fogu → Bun (Fogu.host.emit)
fogu.on("done", (data) => console.log("done:", data)); // done: { id: 7, square: 49 }
fogu.eval(`Fogu.signal.on("job", fn(d) { Fogu.host.emit("done", { id: d.id, square: d.id * d.id }) })`);
fogu.emit("job", { id: 7 });

// 4. Fogu errors become JavaScript exceptions
try {
  fogu.eval("1 / 0");
} catch (e) {
  console.log("error:", (e as Error).message);
}

await Bun.sleep(100); // let the event arrive
fogu.close();
