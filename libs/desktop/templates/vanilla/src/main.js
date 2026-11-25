// The page: call() runs a Fogu command (main.fg), on() receives its events.
import "./style.css";
import { call, emit, on, isDesktop } from "./fogu-desktop.js";

document.querySelector("#app").innerHTML = `
  <main>
    <h1>{{title}}</h1>
    <p class="sub">Vite in front, a Fogu program behind.</p>
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
  </main>`;

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
