// HTTP: GET /hello → a small JSON body (Bun.serve)
const port = Number(process.argv[2] ?? 8004);
Bun.serve({
  port,
  hostname: "127.0.0.1",
  fetch(req) {
    return Response.json({ message: "hello", path: new URL(req.url).pathname });
  },
});
console.log("ready");
