const PANEL = "download";

let url = "";
let status = "Paste a direct link to an audio file.";
let busy = false;

function render() {
  sw.ui.setContent(PANEL, [
    { type: "text", text: status, style: "dim" },
    { type: "space" },
    { type: "input", id: "url", value: url, placeholder: "https://example.com/song.flac" },
    { type: "button", id: "go", label: busy ? "Downloading..." : "Download", primary: !busy },
  ]);
}

function fileName(link) {
  const last = link.split("#")[0].split("?")[0].split("/").pop() || "";
  try {
    return decodeURIComponent(last) || "download";
  } catch (e) {
    return last || "download";
  }
}

function start() {
  if (busy) return;
  if (!/^https?:\/\//i.test(url)) {
    status = "Enter a link that starts with http:// or https://";
    render();
    return;
  }
  busy = true;
  status = "Downloading...";
  render();
  sw.download(url, { filename: fileName(url) })
    .then(function (r) {
      const added = sw.library.add(r.path);
      status = (added ? "Added to your library: " : "Saved (not an audio file): ") + r.path;
      if (added) sw.toast("Added to your library");
    })
    .catch(function (e) {
      status = "Failed: " + e.message;
    })
    .then(function () {
      busy = false;
      render();
    });
}

sw.ui.addPanel({
  id: PANEL,
  title: "Download from URL",
  icon: "import",
  content: [],
  onOpen: render,
  onAction: function (a) {
    if (a.type === "input" && a.id === "url") url = String(a.value).trim();
    else if (a.type === "click" && a.id === "go") start();
  },
});

render();
