const PANEL = "sleep";
const presets = sw.config.presets || [15, 30, 45, 60];
const fadeSeconds = Math.max(0, Number(sw.config.fadeSeconds) || 0);

let endsAt = 0;
let startVolume = 1;
let fade = sw.storage.get("fade", true);
let lastShown = "";

function format(seconds) {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return m + ":" + (s < 10 ? "0" : "") + s;
}

function secondsLeft() {
  return endsAt ? Math.max(0, Math.ceil((endsAt - sw.now()) / 1000)) : 0;
}

function render() {
  const content = [
    { type: "text", text: endsAt ? "Pausing in " + format(secondsLeft()) : "Not running", style: "dim" },
    { type: "space" },
  ];
  presets.forEach(function (m) {
    content.push({ type: "button", id: "start:" + m, label: m + " minutes" });
  });
  if (endsAt) content.push({ type: "button", id: "cancel", label: "Cancel", primary: true });
  content.push({ type: "divider" });
  content.push({ type: "switch", id: "fade", label: "Fade out at the end", value: fade });
  const key = JSON.stringify(content);
  if (key !== lastShown) {
    lastShown = key;
    sw.ui.setContent(PANEL, content);
  }
}

function start(minutes) {
  startVolume = sw.player.getState().volume;
  endsAt = sw.now() + minutes * 60000;
  sw.toast("Sleep timer: " + minutes + " minutes");
  render();
}

function cancel(restoreVolume) {
  if (!endsAt) return;
  endsAt = 0;
  if (restoreVolume) sw.player.setVolume(startVolume);
  render();
}

sw.ui.addPanel({
  id: PANEL,
  title: "Sleep timer",
  icon: "power",
  content: [],
  onOpen: render,
  onAction: function (a) {
    if (a.type === "click" && a.id.indexOf("start:") === 0) start(Number(a.id.slice(6)));
    else if (a.type === "click" && a.id === "cancel") cancel(true);
    else if (a.type === "switch" && a.id === "fade") {
      fade = !!a.value;
      sw.storage.set("fade", fade);
      render();
    }
  },
});

[15, 30, 60].forEach(function (m) {
  sw.commands.register({ id: "start" + m, title: "Sleep timer: " + m + " minutes", run: function () { start(m); } });
});
sw.commands.register({ id: "cancel", title: "Sleep timer: cancel", run: function () { cancel(true); } });

setInterval(function () {
  if (!endsAt) return;
  const left = secondsLeft();
  if (left <= 0) {
    sw.player.pause();
    cancel(true);
    return;
  }
  if (fade && fadeSeconds > 0 && left <= fadeSeconds) {
    sw.player.setVolume(startVolume * (left / fadeSeconds));
  }
  render();
}, 1000);

render();
