// UDP echo server (Bun.udpSocket)
const port = Number(process.argv[2] ?? 9201);
await Bun.udpSocket({ hostname: "127.0.0.1", port, socket: { data(socket, buf, fromPort, fromAddr) { socket.send(buf, fromPort, fromAddr); } } });
console.log("ready");
