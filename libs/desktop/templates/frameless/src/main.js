// A frameless window ("desktop": { "decorations": false, "transparent": true }
// in fogu.json): the page draws its own title bar. `data-fogu-drag` makes an
// element move the window (a double click maximizes it); buttons with
// `data-fogu-window="minimize|maximize|close"` do that, no code needed. The
// same calls exist as functions: fogu.window.minimize(), toggleMaximize(), close().
import "./style.css";
import "./frameless.css";
import { call, emit, on, isDesktop } from "./fogu-desktop.js";

document.querySelector("#app").innerHTML = `
  <div class="frame">
    <header class="titlebar" data-fogu-drag>
      <span class="app-title">{{title}}</span>
      <div class="controls">
        <button type="button" data-fogu-window="minimize" title="Minimize" aria-label="Minimize">&#8211;</button>
        <button type="button" id="max" data-fogu-window="maximize" title="Maximize" aria-label="Maximize">&#9744;</button>
        <button type="button" class="close" data-fogu-window="close" title="Close" aria-label="Close">&#10005;</button>
      </div>
    </header>
    <main>
      <h1>{{title}}</h1>
      <p class="sub">A window without the system title bar: drag the bar above, double-click it to maximize.</p>
      <section>
        <h2>Command</h2>
        <form id="greet">
          <input id="name" placeholder="Your name" autocomplete="off" />
          <button class="primary" type="submit">Greet</button>
        </form>
        <p id="out" class="out"></p>
      </section>
      <section>
        <h2>Events</h2>
        <div class="row">
          <span id="count" class="count">0</span>
          <button id="inc" type="button">+1</button>
          <button id="reset" type="button">Reset</button>
        </div>
      </section>
      <footer id="status"></footer>
    </main>
  </div>`;

const out = document.querySelector("#out");
const status = document.querySelector("#status");

document.querySelector("#greet").addEventListener("submit", async (e) => {
  e.preventDefault();
  try {
    const r = await call("greet", document.querySelector("#name").value);
    out.className = "out";
    out.textContent = r.message;
  } catch (err) {
    out.className = "out error";
    out.textContent = err.message;
  }
});
document.querySelector("#inc").addEventListener("click", () => call("increment", 1));
document.querySelector("#reset").addEventListener("click", () => emit("reset"));

on("count", (n) => (document.querySelector("#count").textContent = String(n)));
on("tick", (s) => (status.textContent = `Fogu backend up for ${s}s`));
status.textContent = isDesktop() ? "Connecting to Fogu…" : "Not in the app: run fogu desktop dev";

// the maximize button shows the state; square corners when maximized
if (isDesktop()) {
  const sync = async () => {
    const max = await window.fogu.window.isMaximized();
    document.body.classList.toggle("maximized", max);
    document.querySelector("#max").innerHTML = max ? "&#10697;" : "&#9744;";
  };
  window.fogu.window.on("resized", sync);
  sync();
}
