# socketioxide 0.18.9, patched for Fogu

This is the published socketioxide 0.18.9 with one change, used by `fogu:socketio`'s
connection state recovery (the `recovery` option of `socketio.server`):

- `Socket::set_recovery_pid(pid)` / `Socket::recovery_pid()` (src/socket.rs): a private
  session id a connect middleware sets;
- the CONNECT packet (src/ns.rs) carries it as `{ "sid": ..., "pid": ... }`, as the
  official Socket.IO server does. The official clients keep it and send it back,
  with the offset of the last broadcast they got, when they reconnect.

Upgrading socketioxide: copy the new version here and apply the same change (search
for "Fogu patch").
