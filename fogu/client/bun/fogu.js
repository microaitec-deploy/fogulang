// src/fogu/fogu.js
import net from "net";

// ============================================================
// Protocol constants
// ============================================================

const Opcode = Object.freeze({
  // ── Control ──────────────────────────────────────────────
  Ping:         0x01,
  Pong:         0x02,
  Auth:         0x50,
  Error:        0xFF,

  // ── KV ───────────────────────────────────────────────────
  Get:          0x10,
  Set:          0x11,
  Del:          0x12,
  Exists:       0x13,
  Incr:         0x20,
  Decr:         0x21,

  // ── Channels (rooms) ─────────────────────────────────────
  Join:         0x30,
  Leave:        0x31,
  ChannelMsg:   0x32,
  ChannelList:  0x33,
  Members:      0x34,

  // ── Topics (ephemeral pub/sub) ────────────────────────────
  Subscribe:    0x40,
  Publish:      0x41,

  // ── RPC ───────────────────────────────────────────────────
  Call:         0x60,
  Act:          0x61,
  Res:          0x62,

  // ── API Key Management (root only) ────────────────────────
  ApiKeyCreate: 0x70,
  ApiKeyDelete: 0x71,
  ApiKeyList:   0x72,
  ApiKeyGet:    0x73,
});

const Status = Object.freeze({
  Ok:           0x00,
  NotFound:     0x01,
  Error:        0x02,
  Invalid:      0x03,
  Unauthorized: 0x04,
  Forbidden:    0x05,
});

const HEADER_SIZE = 17;
const MAGIC_0     = 0x46; // 'F'
const MAGIC_1     = 0x4F; // 'O'
const VERSION     = 0x01;

// ============================================================
// Frame helpers
// ============================================================

function buildFrame(opcode, requestId, payload = Buffer.alloc(0), flags = 0) {
  const buf = Buffer.allocUnsafe(HEADER_SIZE + payload.length);
  buf[0] = MAGIC_0;
  buf[1] = MAGIC_1;
  buf[2] = VERSION;
  buf[3] = opcode;
  buf[4] = flags;
  buf.writeBigUInt64BE(BigInt(requestId), 5);
  buf.writeUInt32BE(payload.length, 13);
  payload.copy(buf, 17);
  return buf;
}

function jsonBuf(obj) {
  return Buffer.from(JSON.stringify(obj), "utf8");
}

function tryParseJson(buf) {
  try   { return JSON.parse(buf.toString("utf8")); }
  catch { return buf.toString("utf8"); }
}

// ============================================================
// URL parser  fogu://token@host:port
// ============================================================

function parseFoguUrl(url) {
  const match = String(url).match(/^fogu:\/\/(?:[^:@]+:)?([^@]+)@([^:]+):(\d+)$/);
  if (!match) throw new Error("URL inválida. Formato esperado: fogu://token@host:port");
  const [, token, host, port] = match;
  return { token, host, port: Number(port) };
}

// ============================================================
// FoguError — carries status code from server
// ============================================================

export class FoguError extends Error {
  constructor(message, status = null) {
    super(message);
    this.name   = "FoguError";
    this.status = status;
  }
}

// ============================================================
// Factory
// ============================================================

export const Fogu = () => {
  const socket = new net.Socket();

  let connected     = false;
  let authenticated = false;
  let isRoot        = false;
  let idCounter     = 1;
  let recvBuffer    = Buffer.alloc(0);

  const pending = {};
  const channelMsgHandlers = {};
  const topicHandlers = {};
  const actionHandlers = {};

  // ── Internals ────────────────────────────────────────────

  function nextId() { return idCounter++; }

  function requireAuth() {
    if (!connected)     throw new FoguError("não conectado");
    if (!authenticated) throw new FoguError("não autenticado");
  }

  function requireRoot() {
    requireAuth();
    if (!isRoot) throw new FoguError("operação exclusiva do root", Status.Forbidden);
  }

  function writeFrame(opcode, requestId, payloadBuf = Buffer.alloc(0), flags = 0) {
    if (!connected) throw new FoguError("não conectado");
    socket.write(buildFrame(opcode, requestId, payloadBuf, flags));
  }

  function makeRequest(opcode, payloadObj = null, timeoutMs = 5000) {
    requireAuth();
    const id  = nextId();
    const buf = payloadObj !== null ? jsonBuf(payloadObj) : Buffer.alloc(0);
    writeFrame(opcode, id, buf);
    return new Promise((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        delete pending[id];
        reject(new FoguError(`timeout (opcode 0x${opcode.toString(16)})`));
      }, timeoutMs);
      pending[id] = { resolve, reject, timeoutId };
    });
  }

  // ── Frame dispatcher ─────────────────────────────────────

  function dispatchFrame(opcode, requestId, payload) {

    if (opcode === Opcode.Ping) {
      writeFrame(Opcode.Pong, requestId);
      return;
    }

    // ── ChannelMsg push ───────────────────────────────────
    if (opcode === Opcode.ChannelMsg) {
      const isAck = pending[requestId] && payload.length > 0 && payload[0] <= 0x05;
      if (!isAck) {
        const envelope = tryParseJson(payload);
        const ch = envelope?.channel;
        for (const fn of channelMsgHandlers[ch] ?? []) {
          try { fn(envelope); } catch (e) { console.warn("[FOGU] channelMsg handler:", e); }
        }
        return;
      }
    }

    // ── Publish push (CORRIGIDO: filtra pelo tópico correto) ────────
    if (opcode === Opcode.Publish && !pending[requestId]) {
      const envelope = tryParseJson(payload);
      
      // Descobre qual é o tópico alvo da mensagem
      // Formato 1: { topic: "channel/topic", data: ... }
      // Formato 2: { topic: "topic", channel: "channel", data: ... }
      let targetKey = envelope?.topic;
      if (!targetKey && envelope?.channel && envelope?.topic) {
        targetKey = `${envelope.channel}/${envelope.topic}`;
      }
      
      const data = envelope?.data !== undefined ? envelope.data : envelope;
      
      // Dispara APENAS para os handlers do tópico correspondente
      const handlers = topicHandlers[targetKey] ?? [];
      for (const fn of handlers) {
        try { fn(data, envelope); } catch (e) { console.warn("[FOGU] topic handler:", e); }
      }
      
      return;
    }

    // ── Call push (RPC handler) ───────────────────────────
    if (opcode === Opcode.Call && !pending[requestId]) {
      let req;

      try   { req = JSON.parse(payload.toString("utf8")); }
      catch { req = { method: null, params: payload.toString("utf8") }; }

      const method = req.method;
      const params = req.params !== undefined ? req.params : req;
      const handler = actionHandlers[method];

      if (handler) {
        try {
          handler(params, {
            method,
            requestId,
            reply(result, error = null) {
              writeFrame(
                Opcode.Res,
                requestId,
                jsonBuf(error ? { result: null, error } : { result, error: null }),
              );
            },
          });
        } catch (e) {
          console.warn(`[FOGU] rpc action '${method}':`, e);
          writeFrame(Opcode.Res, requestId, jsonBuf({ result: null, error: String(e.message) }));
        }
      } else {
        writeFrame(Opcode.Res, requestId, jsonBuf({ result: null, error: `Method '${method}' not found` }));
      }
      return;
    }

    // ── Res (async RPC reply) ─────────────────────────────
    if (opcode === Opcode.Res) {
      const p = pending[requestId];
      if (!p) return;
      clearTimeout(p.timeoutId);
      delete pending[requestId];
      let body;
      try { body = JSON.parse(payload.toString("utf8")); } catch { body = null; }
      if (body?.error) return p.reject(new FoguError(body.error));
      p.resolve(body?.result ?? body);
      return;
    }

    // ── Error / Forbidden frame ───────────────────────────
    if (opcode === Opcode.Error) {
      const p = pending[requestId];
      if (!p) return;
      clearTimeout(p.timeoutId);
      delete pending[requestId];
      const status = payload.length > 0 ? payload[0] : null;
      const msg    = (payload.length > 1 ? payload.subarray(1) : payload).toString("utf8");

      let errMsg = msg;
      try {
        const parsed = JSON.parse(msg);
        if (parsed?.error) errMsg = parsed.error;
      } catch { /* use raw string */ }

      p.reject(new FoguError(errMsg || "erro desconhecido", status));
      return;
    }

    // ── Normal reply ──────────────────────────────────────
    const p = pending[requestId];
    if (!p) return;
    clearTimeout(p.timeoutId);
    delete pending[requestId];

    if (payload.length === 0) {
      p.resolve({ status: Status.Ok, data: null });
      return;
    }

    const status = payload[0];
    const body   = payload.subarray(1);

    if (status === Status.Ok || status === Status.NotFound) {
      let result = null;
      if (body.length > 0) {
        try   { result = JSON.parse(body.toString("utf8")); }
        catch { result = body.toString("utf8"); }
      }
      p.resolve({ status, data: result });
    } else {
      let errMsg = body.toString("utf8");
      try {
        const parsed = JSON.parse(errMsg);
        if (parsed?.error) errMsg = parsed.error;
      } catch { /* use raw */ }
      p.reject(new FoguError(errMsg || `status 0x${status.toString(16)}`, status));
    }
  }

  // ── Buffer processor ──────────────────────────────────────

  function processBuffer() {
    while (recvBuffer.length >= HEADER_SIZE) {
      if (recvBuffer[0] !== MAGIC_0 || recvBuffer[1] !== MAGIC_1) {
        console.error("[FOGU] magic inválido — buffer descartado");
        recvBuffer = Buffer.alloc(0);
        break;
      }
      const payloadLen = recvBuffer.readUInt32BE(13);
      const totalLen   = HEADER_SIZE + payloadLen;
      if (recvBuffer.length < totalLen) break;

      const opcode    = recvBuffer[3];
      const requestId = Number(recvBuffer.readBigUInt64BE(5));
      const payload   = recvBuffer.subarray(17, totalLen);
      recvBuffer      = recvBuffer.subarray(totalLen);

      dispatchFrame(opcode, requestId, payload);
    }
  }

  // ── Socket events ─────────────────────────────────────────

  socket.on("data", (chunk) => {
    recvBuffer = Buffer.concat([recvBuffer, chunk]);
    processBuffer();
  });

  socket.on("close", () => {
    connected = authenticated = isRoot = false;
    for (const [id, p] of Object.entries(pending)) {
      clearTimeout(p.timeoutId);
      p.reject(new FoguError("conexão encerrada"));
      delete pending[id];
    }
  });

  socket.on("error", (err) => {
    connected = authenticated = isRoot = false;
    console.error("[FOGU] socket error:", err.message);
  });

  // ============================================================
  // Public API
  // ============================================================

  return {
    get connected()     { return connected;     },
    get authenticated() { return authenticated; },
    get isRoot()        { return isRoot;        },

    async connect(url, timeoutMs = 30000) {
      const { token, host, port } = parseFoguUrl(url);

      await new Promise((resolve, reject) => {
        const onError   = (err) => { socket.off("connect", onConnect); reject(err); };
        const onConnect = ()    => { socket.off("error",   onError);   connected = true; resolve(); };
        socket.once("error",   onError);
        socket.once("connect", onConnect);
        socket.connect(port, host);
      });

      const authId = nextId();
      writeFrame(Opcode.Auth, authId, jsonBuf({ token }));

      await new Promise((resolve, reject) => {
        const timeoutId = setTimeout(() => {
          delete pending[authId];
          socket.destroy();
          reject(new FoguError("timeout aguardando AUTH"));
        }, timeoutMs);
        pending[authId] = {
          resolve: (res) => {
            authenticated = true;
            isRoot        = res?.data?.result?.role === "root";
            resolve();
          },
          reject: (err) => {
            connected = false;
            socket.destroy();
            reject(err);
          },
          timeoutId,
        };
      });
    },

    disconnect() {
      connected = authenticated = isRoot = false;
      socket.end();
    },

    ping(timeoutMs = 5000) {
      requireAuth();
      const id = nextId();
      writeFrame(Opcode.Ping, id);
      return new Promise((resolve, reject) => {
        const timeoutId = setTimeout(() => {
          delete pending[id];
          reject(new FoguError("ping timeout"));
        }, timeoutMs);
        pending[id] = { resolve, reject, timeoutId };
      });
    },

    kv: {
      get(key, timeoutMs = 5000) {
        return makeRequest(Opcode.Get, { key }, timeoutMs).then(r => r.data?.value ?? null);
      },
      set(key, value, ttl = null, timeoutMs = 5000) {
        return makeRequest(Opcode.Set, ttl !== null ? { key, value, ttl } : { key, value }, timeoutMs).then(() => true);
      },
      del(key, timeoutMs = 5000) {
        return makeRequest(Opcode.Del, { key }, timeoutMs).then(r => (r.data?.count ?? 0) > 0);
      },
      exists(key, timeoutMs = 5000) {
        return makeRequest(Opcode.Exists, { key }, timeoutMs).then(r => r.data?.exists ?? false);
      },
      incr(key, delta = 1, timeoutMs = 5000) {
        return makeRequest(Opcode.Incr, { key, delta }, timeoutMs).then(r => r.data?.value ?? 0);
      },
      decr(key, delta = 1, timeoutMs = 5000) {
        return makeRequest(Opcode.Decr, { key, delta }, timeoutMs).then(r => r.data?.value ?? 0);
      },
    },

    apikeys: {
      create(name, perms = {}, timeoutMs = 5000) {
        requireRoot();
        const payload = {
          name,
          channels: perms.channels ?? [],
          kv:       perms.kv       ?? [],
          topics:   perms.topics   ?? [],
          rpc:      perms.rpc      ?? [],
        };
        return makeRequest(Opcode.ApiKeyCreate, payload, timeoutMs)
          .then(r => r.data?.api_key ?? r.data);
      },
      delete(id, timeoutMs = 5000) {
        requireRoot();
        return makeRequest(Opcode.ApiKeyDelete, { id }, timeoutMs)
          .then(r => r.data?.result ?? r.data);
      },
      list(timeoutMs = 5000) {
        requireRoot();
        return makeRequest(Opcode.ApiKeyList, null, timeoutMs)
          .then(r => r.data?.api_keys ?? []);
      },
      get(id, timeoutMs = 5000) {
        requireRoot();
        return makeRequest(Opcode.ApiKeyGet, { id }, timeoutMs)
          .then(r => r.data?.api_key ?? r.data);
      },
    },

    channel(name) {
      const ns = (sub) => sub ? `${name}/${sub}` : name;

      return {
        join(alias = null, timeoutMs = 5000) {
          return makeRequest(Opcode.Join, alias ? { channel: name, alias } : { channel: name }, timeoutMs)
            .then(r => r.data?.result ?? r.data);
        },
        leave(timeoutMs = 5000) {
          return makeRequest(Opcode.Leave, { channel: name }, timeoutMs)
            .then(r => r.data?.result ?? r.data);
        },
        send(data, timeoutMs = 5000) {
          return makeRequest(Opcode.ChannelMsg, { channel: name, data }, timeoutMs)
            .then(() => true);
        },
        onMessage(fn) {
          if (!channelMsgHandlers[name]) channelMsgHandlers[name] = [];
          channelMsgHandlers[name].push(fn);
          return () => {
            channelMsgHandlers[name] = (channelMsgHandlers[name] ?? []).filter(f => f !== fn);
            if (!channelMsgHandlers[name].length) delete channelMsgHandlers[name];
          };
        },
        members(timeoutMs = 5000) {
          return makeRequest(Opcode.Members, { channel: name }, timeoutMs)
            .then(r => r.data?.members ?? []);
        },
        list(timeoutMs = 5000) {
          return makeRequest(Opcode.ChannelList, null, timeoutMs)
            .then(r => r.data?.channels ?? []);
        },

        // ── Topics (pub/sub, namespaced) ──────────────────────
        async subscribe(topic, fn, timeoutMs = 5000) {
          const key = ns(topic);
          if (!topicHandlers[key]) topicHandlers[key] = [];
          topicHandlers[key].push(fn);
          await makeRequest(Opcode.Subscribe, { topic: key }, timeoutMs);
          return () => {
            topicHandlers[key] = (topicHandlers[key] ?? []).filter(f => f !== fn);
            if (!topicHandlers[key].length) delete topicHandlers[key];
          };
        },

        publish(topic, data, timeoutMs = 5000) {
          return makeRequest(Opcode.Publish, { topic: ns(topic), data }, timeoutMs)
            .then(() => true);
        },

        // ── RPC (namespaced) ──────────────────────────────────
        act(method, fn, timeoutMs = 5000) {
          const key = ns(method);
          actionHandlers[key] = fn;
          return makeRequest(Opcode.Act, { method: key }, timeoutMs)
            .then(() => true);
        },
        call(method, params = {}, timeoutMs = 5000) {
          requireAuth();
          const key = ns(method);
          const id  = nextId();
          writeFrame(Opcode.Call, id, jsonBuf({ method: key, params }));
          return new Promise((resolve, reject) => {
            const timeoutId = setTimeout(() => {
              delete pending[id];
              reject(new FoguError(`timeout aguardando resposta de '${key}'`));
            }, timeoutMs);
            pending[id] = { resolve, reject, timeoutId };
          });
        },
      };
    },
  };
};