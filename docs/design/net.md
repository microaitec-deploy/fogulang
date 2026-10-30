# Design note: `fogu:net`

Source: [`src/native/net.rs`](../../src/native/net.rs).

Four native objects — `TcpListener`, `TcpStream`, `UdpSocket`, `UdpPacket` —
and factory functions in the `fogu:net` module.

## API

```fogu
import net from "fogu:net"

// TCP server
const server = net.tcpListen("127.0.0.1:9000")?
for conn in server {                  // accepts connections forever (use break to stop)
    const line = conn.readLine()?     // nil at end of stream
    conn.writeLine(`received: ${line}`)
    conn.close()
}

// TCP client
const c = net.tcpConnect("127.0.0.1:9000")?
c.writeLine("hello")
print(c.readLine())
c.close()

// UDP
const sock = net.udpBind("127.0.0.1:0")?
sock.sendTo("127.0.0.1:9001", "ping")
const packet = sock.recvFrom()        // packet.data (buffer), packet.text, packet.from, packet.len
print(net.resolve("localhost"))       // list of IPs
```

`TcpStream` also has `read(n = 64 KiB)` / `readExact(n)` / `readAll()` (→ `buffer`),
`readText(n)`, `write`/`writeLine` (→ bytes written), `setTimeout(d)`, `peer`,
`localAddr`, `open`; `UdpSocket` has `connect`, `send`, `recv(n)` (→ buffer),
`setTimeout`; `TcpListener` has `accept(timeout?)`, `setTimeout`, `addr`, `port`.

## Notes and limitations

1. **Cooperative waits**: every operation that waits runs on the shared tokio
   runtime (`rt::tokio()`); the calling task waits cooperatively and the other
   `fg` tasks keep running. Completion wakes the scheduler (no polling).
2. **`for conn in server`**: the interpreter iterates a `TcpListener` by calling
   `accept()` repeatedly (spec §47).
3. **Line reads**: each stream keeps one read buffer, so `readLine` never loses the
   bytes that arrived after the line. `readLine`/`read` return `nil` at the end of
   the stream.
4. **No TLS** yet.
5. **Bytes**: binary data is a `buffer` (`read`, `readExact`, `readAll`, `recv`,
   `UdpPacket.data`); `readLine`/`readText`/`UdpPacket.text` decode UTF-8.
   Writes accept a `str`, a `buffer` or a `list<int>`.
6. **Errors**: connection/listen/bind/read/write failures and timeouts are
   operational errors, returned as `error` values.
