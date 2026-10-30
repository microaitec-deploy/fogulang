// WebSocket echo server (Bun.serve)
const port = Number(process.argv[2] ?? 9003);
Bun.serve({
  port,
  hostname: "127.0.0.1",
  fetch(req, server) {
    if (server.upgrade(req)) return;
    return new Response("websocket only", { status: 400 });
  },
  websocket: { message(ws, msg) { ws.send(msg); }, perMessageDeflate: false },
});
console.log("ready");
