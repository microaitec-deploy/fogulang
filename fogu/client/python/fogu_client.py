"""Minimal FoguMQ client (stdlib only): auth, kv, call/act, subscribe/publish."""
import json, socket, struct, threading, queue, itertools

OP = dict(PING=1, PONG=2, GET=0x10, SET=0x11, DEL=0x12, EXISTS=0x13, INCR=0x20, DECR=0x21, JOIN=0x30,
          SEND=0x32, SUB=0x40, PUB=0x41, AUTH=0x50, CALL=0x60, ACT=0x61, RES=0x62, ERROR=0xFF)

def frame(op, rid, body=b""):
    return b"FO" + bytes([1, op, 0]) + struct.pack(">QI", rid, len(body)) + body

class Client:
    def __init__(self, host, port, token):
        self.s = socket.create_connection((host, port))
        self.ids = itertools.count(1)
        self.waiting, self.lock = {}, threading.Lock()
        self.topics, self.actions = {}, {}
        threading.Thread(target=self._read, daemon=True).start()
        r = self._req(OP["AUTH"], {"token": token})
        self.role = r["result"]["role"]

    def _recv(self, n):
        b = b""
        while len(b) < n:
            chunk = self.s.recv(n - len(b))
            if not chunk:
                raise EOFError
            b += chunk
        return b

    def _read(self):
        try:
            while True:
                h = self._recv(17)
                op, rid, ln = h[3], struct.unpack(">Q", h[5:13])[0], struct.unpack(">I", h[13:17])[0]
                p = self._recv(ln) if ln else b""
                if op == OP["PUB"] and rid not in self.waiting:
                    env = json.loads(p)
                    for fn in self.topics.get(env.get("topic"), []):
                        fn(env.get("data"))
                elif op == OP["CALL"]:
                    req = json.loads(p)
                    fn = self.actions.get(req["method"])
                    res = {"result": fn(req["params"]) if fn else None, "error": None if fn else "not found"}
                    self.s.sendall(frame(OP["RES"], rid, json.dumps(res).encode()))
                elif rid in self.waiting:
                    self.waiting.pop(rid).put((op, p))
        except EOFError:
            pass

    def _req(self, op, body=None, timeout=5):
        rid = next(self.ids)
        q = queue.Queue()
        self.waiting[rid] = q
        self.s.sendall(frame(op, rid, json.dumps(body).encode() if body is not None else b""))
        rop, p = q.get(timeout=timeout)
        if rop == OP["RES"]:
            r = json.loads(p)
            if r.get("error"):
                raise RuntimeError(r["error"])
            return r.get("result")
        if rop == OP["ERROR"] or p[:1] != b"\x00":
            raise RuntimeError(p[1:].decode(errors="replace"))
        return json.loads(p[1:]) if len(p) > 1 else None

    def kv_set(self, k, v, ttl=None): return self._req(OP["SET"], {"key": k, "value": v, **({"ttl": ttl} if ttl else {})})
    def kv_get(self, k): return self._req(OP["GET"], {"key": k})["value"]
    def kv_incr(self, k, d=1): return self._req(OP["INCR"], {"key": k, "delta": d})["value"]
    def call(self, method, params): return self._req(OP["CALL"], {"method": method, "params": params})
    def act(self, method, fn):
        self.actions[method] = fn
        return self._req(OP["ACT"], {"method": method})
    def subscribe(self, topic, fn):
        self.topics.setdefault(topic, []).append(fn)
        return self._req(OP["SUB"], {"topic": topic})
    def publish(self, topic, data): return self._req(OP["PUB"], {"topic": topic, "data": data})
