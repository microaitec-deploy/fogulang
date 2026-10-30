// memory per task: N async functions blocked on a promise, then all released
const n = Number(process.argv[2] ?? 100000);
let open;
const gate = new Promise((r) => (open = r));
const tasks = Array.from({ length: n }, async () => { await gate; });
await new Promise((r) => setTimeout(r, 50));
open();
await Promise.all(tasks);
console.log(n);
