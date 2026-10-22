(function (g) {
  var N = g.__native;
  g.__native = undefined;

  var handlers = {};
  var timers = [];
  var nextTimer = 1;
  var state = { title: "", artist: "", album: "", year: "", duration: 0, position: 0, playing: false };
  var stateAt = N.now();

  function fmt(args) {
    var out = [];
    for (var i = 0; i < args.length; i++) {
      var a = args[i];
      if (typeof a === "string") out.push(a);
      else {
        try { out.push(JSON.stringify(a)); } catch (e) { out.push(String(a)); }
      }
    }
    return out.join(" ");
  }

  function guard(what, fn, arg) {
    try {
      fn(arg);
    } catch (e) {
      N.log("error", what + ": " + String(e) + (e && e.stack ? String.fromCharCode(10) + e.stack : ""));
    }
  }

  function snapshot() {
    var s = {};
    for (var k in state) s[k] = state[k];
    if (s.playing) {
      var p = state.position + (N.now() - stateAt) / 1000;
      s.position = s.duration > 0 ? Math.min(s.duration, p) : p;
    }
    return s;
  }

  var utf8 = {
    encode: function (str) {
      var out = [];
      for (var i = 0; i < str.length; i++) {
        var c = str.charCodeAt(i);
        if (c >= 0xd800 && c < 0xdc00 && i + 1 < str.length) {
          var d = str.charCodeAt(i + 1);
          if (d >= 0xdc00 && d < 0xe000) {
            c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
            i++;
          }
        }
        if (c < 0x80) out.push(c);
        else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
        else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
        else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      }
      return out;
    },
    decode: function (bytes) {
      var s = "";
      var i = 0;
      while (i < bytes.length) {
        var b = bytes[i++];
        var c;
        if (b < 0x80) c = b;
        else if (b < 0xe0) c = ((b & 31) << 6) | (bytes[i++] & 63);
        else if (b < 0xf0) { c = ((b & 15) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63); }
        else {
          c = ((b & 7) << 18) | ((bytes[i++] & 63) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63);
        }
        if (c >= 0x10000) {
          c -= 0x10000;
          s += String.fromCharCode(0xd800 + (c >> 10), 0xdc00 + (c & 1023));
        } else s += String.fromCharCode(c);
      }
      return s;
    },
  };

  var sw = {
    api: 1,
    version: N.version(),
    pid: N.pid(),
    config: JSON.parse(N.config()),
    utf8: utf8,
    on: function (event, cb) {
      if (typeof cb !== "function") throw new TypeError("sw.on: callback must be a function");
      (handlers[event] = handlers[event] || []).push(cb);
    },
    off: function (event, cb) {
      var l = handlers[event] || [];
      var i = l.indexOf(cb);
      if (i >= 0) l.splice(i, 1);
    },
    log: function () { N.log("info", fmt(arguments)); },
    toast: function (msg) { N.toast(String(msg)); },
    now: function () { return N.now(); },
    player: { getState: snapshot },
    storage: {
      get: function (key, fallback) {
        var v = N.storage_get(String(key));
        if (v === undefined || v === null) return fallback;
        try { return JSON.parse(v); } catch (e) { return fallback; }
      },
      set: function (key, value) { return N.storage_set(String(key), JSON.stringify(value)); },
    },
    pipe: {
      open: function (name) {
        var id = N.pipe_open(String(name));
        return id < 0 ? null : id;
      },
      write: function (id, bytes) { return N.pipe_write(id, Array.prototype.slice.call(bytes)); },
      read: function (id) {
        var r = N.pipe_read(id);
        return r === undefined || r === null ? null : r;
      },
      close: function (id) { N.pipe_close(id); },
    },
  };

  g.sw = sw;
  g.console = {
    log: function () { N.log("info", fmt(arguments)); },
    info: function () { N.log("info", fmt(arguments)); },
    warn: function () { N.log("warn", fmt(arguments)); },
    error: function () { N.log("error", fmt(arguments)); },
  };

  function addTimer(fn, ms, repeat) {
    var id = nextTimer++;
    var every = Math.max(10, Number(ms) || 0);
    timers.push({ id: id, fn: fn, at: N.now() + every, every: every, repeat: repeat });
    return id;
  }
  g.setTimeout = function (fn, ms) { return addTimer(fn, ms, false); };
  g.setInterval = function (fn, ms) { return addTimer(fn, ms, true); };
  g.clearTimeout = g.clearInterval = function (id) {
    timers = timers.filter(function (t) { return t.id !== id; });
  };

  g.__dispatch = function (name, json) {
    var payload = JSON.parse(json);
    if (payload.state) {
      for (var k in payload.state) state[k] = payload.state[k];
      stateAt = N.now();
    }
    var list = (handlers[name] || []).slice();
    for (var i = 0; i < list.length; i++) guard("on(" + name + ")", list[i], snapshot());
  };

  g.__tick = function () {
    var now = N.now();
    var due = timers.filter(function (t) { return t.at <= now; });
    for (var i = 0; i < due.length; i++) {
      var t = due[i];
      if (t.repeat) t.at = now + t.every;
      else timers = timers.filter(function (x) { return x.id !== t.id; });
      guard("timer", t.fn);
    }
  };
})(globalThis);
