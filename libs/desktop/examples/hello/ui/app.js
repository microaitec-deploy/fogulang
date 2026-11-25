// The page: window.fogu is injected by the fogu-desktop host.
//   fogu.call(name, payload) → the Fogu command's value (a Promise)
//   fogu.on(name, fn)        → desktop.emit(name, data) in Fogu
//   fogu.emit(name, data)    → desktop.on(name, fn) in Fogu
const $ = (id) => document.getElementById(id);

function render({ items, stats }) {
  const list = $("list");
  list.replaceChildren(
    ...items.map((item) => {
      const li = document.createElement("li");
      li.className = item.done ? "done" : "";
      const label = document.createElement("label");
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = item.done;
      box.addEventListener("change", () => run("toggle", { id: item.id }));
      const text = document.createElement("span");
      text.textContent = item.text;
      label.append(box, text);
      const del = document.createElement("button");
      del.title = "Remove";
      del.textContent = "×";
      del.addEventListener("click", () => run("remove", { id: item.id }));
      li.append(label, del);
      return li;
    }),
  );
  $("empty").hidden = items.length > 0;
  $("stats").textContent = stats.total ? `${stats.left} left · ${stats.done} done` : "";
}

function showError(message) {
  const el = $("error");
  el.textContent = message || "";
  el.hidden = !message;
}

let toastTimer;
function toast(message) {
  const el = $("toast");
  el.textContent = message;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), 2500);
}

// runs a command; a Fogu error (return nil, error("…")) rejects with its message
async function run(name, payload) {
  try {
    showError("");
    return await fogu.call(name, payload);
  } catch (e) {
    showError(e.message);
    return undefined;
  }
}

$("add").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const input = $("text");
  const item = await run("add", { text: input.value });
  if (item) {
    input.value = "";
  }
  input.focus();
});
$("clear").addEventListener("click", () => fogu.emit("clear-done"));
$("export").addEventListener("click", async () => {
  const path = await run("export");
  if (path) {
    toast(`Saved to ${path}`);
  }
});

// events from Fogu
fogu.on("todos", render);
fogu.on("toast", toast);
fogu.on("uptime", (s) => ($("uptime").textContent = s));

// first paint
run("list").then((data) => data && render(data));
run("about").then((a) => a && ($("about").textContent = `Fogu ${a.fogu} on ${a.os}`));
