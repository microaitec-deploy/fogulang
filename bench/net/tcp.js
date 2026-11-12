// TCP echo server (Bun.listen)
const port = Number(process.argv[2] ?? 9101);
Bun.listen({ hostname: "127.0.0.1", port, socket: { data(socket, data) { socket.write(data); } } });
console.log("ready");
