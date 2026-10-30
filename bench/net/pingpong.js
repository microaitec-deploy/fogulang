// message passing: 100k round trips between two tasks over queues
// (JavaScript has no channels: a minimal promise queue)
class Queue {
  items = []; waiters = [];
  put(v) { const w = this.waiters.shift(); if (w) w(v); else this.items.push(v); }
  get() { return this.items.length ? Promise.resolve(this.items.shift()) : new Promise((r) => this.waiters.push(r)); }
}
const ping = new Queue(), pong = new Queue();
(async () => {
  while (true) {
    const n = await ping.get();
    if (n === null) break;
    pong.put(n + 1);
  }
})();
let total = 0;
for (let i = 0; i < 100000; i++) {
  ping.put(i);
  total += (await pong.get()) - i;
}
ping.put(null);
console.log(total);
