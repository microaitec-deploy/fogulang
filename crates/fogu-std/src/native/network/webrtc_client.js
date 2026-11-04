// FoguRTC — the browser side of fogu:webrtc. Served by `webrtc.signaling(app, path)`
// at `<path>/client.js` (and exported as `webrtc.clientJs`).
//
//   <script src="/rtc/client.js"></script>
//   const conn = await FoguRTC.connect("/rtc", "room-1")    // ws(s)://… or a path on this host
//   conn.channel.onmessage = (e) => console.log(e.data)     // str, or ArrayBuffer for binary
//   conn.send("hello")
//
//   const conn2 = await FoguRTC.connectHttp("/rtc/offer")    // webrtc.serveHttp: one POST
//
// Both sides gather their candidates before sending the description, so one
// offer/answer exchange is enough; candidates found later are trickled too.
(function (global) {
  "use strict";
  const DEFAULT_ICE = [{ urls: "stun:stun.l.google.com:19302" }];

  function wsUrl(url) {
    if (/^wss?:\/\//.test(url)) return url;
    const loc = global.location;
    return (loc.protocol === "https:" ? "wss://" : "ws://") + loc.host + (url.startsWith("/") ? url : "/" + url);
  }

  function gathered(pc, ms) {
    if (pc.iceGatheringState === "complete") return Promise.resolve();
    return new Promise((resolve) => {
      const done = () => { pc.removeEventListener("icegatheringstatechange", check); resolve(); };
      const check = () => { if (pc.iceGatheringState === "complete") done(); };
      pc.addEventListener("icegatheringstatechange", check);
      setTimeout(done, ms);
    });
  }

  function opened(channel, ms) {
    if (channel.readyState === "open") return Promise.resolve(channel);
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error("fogu-webrtc: the channel did not open")), ms);
      channel.addEventListener("open", () => { clearTimeout(t); resolve(channel); }, { once: true });
    });
  }

  function result(pc, channel) {
    channel.binaryType = "arraybuffer";
    return {
      pc,
      channel,
      send: (data) => channel.send(data),
      close: () => { channel.close(); pc.close(); },
    };
  }

  async function connect(url, room, opts) {
    opts = opts || {};
    const label = opts.label || "data";
    const timeout = opts.timeout || 30000;
    const ws = new WebSocket(wsUrl(url));
    const inbox = [];
    let waiting = null;
    let pc = null;
    let partner = null;
    ws.onmessage = (e) => {
      let m;
      try { m = JSON.parse(e.data); } catch (_) { return; }
      if (m.type === "candidate" && pc && partner && m.from === partner) {
        if (m.candidate) pc.addIceCandidate({ candidate: m.candidate, sdpMid: m.sdpMid || "0", sdpMLineIndex: m.sdpMLineIndex || 0 }).catch(() => {});
        return;
      }
      inbox.push(m);
      if (waiting) waiting();
    };
    const next = (pred, ms) => new Promise((resolve, reject) => {
      const t = ms ? setTimeout(() => { waiting = null; reject(new Error("fogu-webrtc: timed out")); }, ms) : null;
      const look = () => {
        const i = inbox.findIndex(pred);
        if (i < 0) return false;
        if (t) clearTimeout(t);
        waiting = null;
        resolve(inbox.splice(i, 1)[0]);
        return true;
      };
      if (!look()) waiting = look;
    });
    await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = () => reject(new Error("fogu-webrtc: cannot reach " + url)); });
    const send = (m) => ws.send(JSON.stringify(m));
    send({ type: "join", room });
    const joined = await next((m) => m.type === "joined", 10000);
    pc = new RTCPeerConnection({ iceServers: opts.iceServers || DEFAULT_ICE });
    pc.onicecandidate = (e) => {
      if (e.candidate && e.candidate.candidate && partner) {
        send({ type: "candidate", to: partner, candidate: e.candidate.candidate, sdpMid: e.candidate.sdpMid, sdpMLineIndex: e.candidate.sdpMLineIndex });
      }
    };
    let channel;
    if (joined.peers.length > 0) {
      partner = joined.peers[0];
      channel = pc.createDataChannel(label, opts.channel || {});
      await pc.setLocalDescription(await pc.createOffer());
      await gathered(pc, 2000);
      send({ type: "offer", to: partner, sdp: pc.localDescription.sdp });
      const answer = await next((m) => m.type === "answer" && m.from === partner, timeout);
      await pc.setRemoteDescription({ type: "answer", sdp: answer.sdp });
    } else {
      const incoming = new Promise((resolve) => { pc.ondatachannel = (e) => resolve(e.channel); });
      const offer = await next((m) => m.type === "offer", opts.waitTimeout || 0);
      partner = offer.from;
      await pc.setRemoteDescription({ type: "offer", sdp: offer.sdp });
      await pc.setLocalDescription(await pc.createAnswer());
      await gathered(pc, 2000);
      send({ type: "answer", to: partner, sdp: pc.localDescription.sdp });
      channel = await incoming;
    }
    await opened(channel, 15000);
    ws.close();
    return result(pc, channel);
  }

  async function connectHttp(url, opts) {
    opts = opts || {};
    const pc = new RTCPeerConnection({ iceServers: opts.iceServers || DEFAULT_ICE });
    const channel = pc.createDataChannel(opts.label || "data", opts.channel || {});
    await pc.setLocalDescription(await pc.createOffer());
    await gathered(pc, 2000);
    const res = await fetch(url, { method: "POST", headers: { "content-type": "application/sdp" }, body: pc.localDescription.sdp });
    if (!res.ok) throw new Error("fogu-webrtc: " + res.status + " " + (await res.text()));
    await pc.setRemoteDescription({ type: "answer", sdp: await res.text() });
    await opened(channel, 15000);
    return result(pc, channel);
  }

  global.FoguRTC = { connect, connectHttp };
})(typeof window !== "undefined" ? window : globalThis);
