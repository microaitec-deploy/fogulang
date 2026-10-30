// concurrency: 10k tasks, each waits 100 ms then adds to a counter
let done = 0;
await Promise.all(Array.from({ length: 10000 }, async () => {
  await new Promise((r) => setTimeout(r, 100));
  done++;
}));
console.log(done);
