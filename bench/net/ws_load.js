// The WebSocket load (the same for every server): `connections` clients, each one
// sends a message and waits for its echo, again and again, for `seconds`.
// Prints {"messages": n, "seconds": s, "errors": e} as JSON.
const [url, connections = "50", seconds = "5"] = process.argv.slice(2);
const conns = Number(connections), duration = Number(seconds) * 1000;
let messages = 0, errors = 0;
const payload = JSON.stringify({ kind: "echo", text: "x".repeat(64) });
const start = performance.now();
await Promise.all(Array.from({ length: conns }, () => new Promise((resolve) => {
  const ws = new WebSocket(url);
  ws.onopen = () => ws.send(payload);
  ws.onmessage = (ev) => {
    if (ev.data !== payload) errors++;
    messages++;
    if (performance.now() - start < duration) ws.send(payload); else { ws.close(); resolve(); }
  };
  ws.onerror = () => { errors++; resolve(); };
})));
console.log(JSON.stringify({ messages, seconds: (performance.now() - start) / 1000, errors }));
