const OP_HANDSHAKE = 0;
const OP_FRAME = 1;
const OP_CLOSE = 2;
const OP_PING = 3;
const OP_PONG = 4;

const RECONNECT_MS = 10000;
const MIN_UPDATE_MS = 1500;

const config = sw.config;
const base = config.pipeBase || "discord-ipc-";

let pipe = null;
let ready = false;
let inbox = [];
let nextTry = 0;
let lastSend = 0;
let dirty = false;
let warned = false;

function u32(n) {
  return [n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255];
}

function frame(op, payload) {
  const body = sw.utf8.encode(typeof payload === "string" ? payload : JSON.stringify(payload));
  return u32(op).concat(u32(body.length), body);
}

function send(op, payload) {
  return pipe !== null && sw.pipe.write(pipe, frame(op, payload));
}

function disconnect() {
  if (pipe !== null) sw.pipe.close(pipe);
  pipe = null;
  ready = false;
  inbox = [];
  nextTry = sw.now() + RECONNECT_MS;
}

function connect() {
  if (!config.clientId) {
    if (!warned) {
      warned = true;
      sw.toast("Discord Rich Presence: set clientId in the plugin's config.json");
    }
    return;
  }
  for (let i = 0; i < 10; i++) {
    const id = sw.pipe.open(base + i);
    if (id === null) continue;
    pipe = id;
    if (send(OP_HANDSHAKE, { v: 1, client_id: String(config.clientId) })) return;
    disconnect();
    return;
  }
  nextTry = sw.now() + RECONNECT_MS;
}

function readFrames() {
  const chunk = sw.pipe.read(pipe);
  if (chunk === null) {
    disconnect();
    return;
  }
  inbox = inbox.concat(chunk);
  while (inbox.length >= 8) {
    const op = inbox[0] | (inbox[1] << 8) | (inbox[2] << 16) | (inbox[3] << 24);
    const len = (inbox[4] | (inbox[5] << 8) | (inbox[6] << 16) | (inbox[7] << 24)) >>> 0;
    if (inbox.length < 8 + len) return;
    const text = sw.utf8.decode(inbox.slice(8, 8 + len));
    inbox = inbox.slice(8 + len);
    if (op === OP_PING) send(OP_PONG, text);
    else if (op === OP_CLOSE) {
      disconnect();
      return;
    } else if (op === OP_FRAME) {
      let msg = {};
      try {
        msg = JSON.parse(text);
      } catch (e) {}
      if (msg.evt === "READY") {
        ready = true;
        dirty = true;
      } else if (msg.evt === "ERROR") {
        sw.log("discord error:", text);
      }
    }
  }
}

function clip(s, max) {
  s = String(s || "");
  if (s.length < 2) s = s + "  ";
  return s.length > max ? s.slice(0, max - 1) + "…" : s;
}

function buildActivity(s) {
  if (!s.title) return null;
  if (!s.playing && !config.showWhenPaused) return null;
  const activity = {
    type: 2,
    details: clip(s.title, 128),
    state: clip(s.playing ? s.artist || "Unknown artist" : "Paused", 128),
    instance: false,
  };
  if (config.largeImage) {
    activity.assets = { large_image: config.largeImage };
    if (s.album) activity.assets.large_text = clip(s.album, 128);
  }
  if (s.playing) {
    const now = Math.floor(sw.now() / 1000);
    const start = Math.floor(now - s.position);
    activity.timestamps = { start: start };
    if (s.duration > 0) activity.timestamps.end = Math.floor(start + s.duration);
  }
  return activity;
}

function publish() {
  if (!ready) return;
  const activity = buildActivity(sw.player.getState());
  const nonce = String(sw.now()) + Math.random();
  send(OP_FRAME, { cmd: "SET_ACTIVITY", args: { pid: sw.pid, activity: activity }, nonce: nonce });
  lastSend = sw.now();
  dirty = false;
}

["trackchange", "play", "pause", "seek"].forEach(function (name) {
  sw.on(name, function () {
    dirty = true;
  });
});

sw.on("stop", function () {
  if (ready) {
    send(OP_FRAME, { cmd: "SET_ACTIVITY", args: { pid: sw.pid, activity: null }, nonce: "stop" });
  }
  disconnect();
});

setInterval(function () {
  if (pipe === null) {
    if (sw.now() >= nextTry) connect();
    return;
  }
  readFrames();
  if (pipe !== null && ready && dirty && sw.now() - lastSend >= MIN_UPDATE_MS) publish();
}, 250);
