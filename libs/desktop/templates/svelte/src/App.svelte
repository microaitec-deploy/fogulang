<!-- The page: call() runs a Fogu command (main.fg), on() receives its events. -->
<script lang="ts">
  import { onDestroy } from "svelte";
  import { call, emit, on, isDesktop } from "./fogu-desktop.js";

  type Greeting = { message: string };

  let name = $state("");
  let answer = $state<{ text: string; error: boolean } | null>(null);
  let count = $state(0);
  let uptime = $state<number | null>(null);
  const inApp = isDesktop();

  const offs = [on<number>("count", (n) => (count = n)), on<number>("tick", (s) => (uptime = s))];
  onDestroy(() => offs.forEach((off) => off()));

  async function greet(e: SubmitEvent) {
    e.preventDefault();
    try {
      const r = await call<Greeting>("greet", name);
      answer = { text: r.message, error: false };
    } catch (err) {
      answer = { text: (err as Error).message, error: true };
    }
  }
</script>

<main>
  <h1>{{title}}</h1>
  <p class="sub">Vite + Svelte in front, a Fogu program behind.</p>
  <section>
    <h2>Command</h2>
    <form onsubmit={greet}>
      <input bind:value={name} placeholder="Your name" autocomplete="off" />
      <button class="primary" type="submit">Greet</button>
    </form>
    <p class={answer?.error ? "out error" : "out"}>{answer?.text}</p>
  </section>
  <section>
    <h2>Events</h2>
    <div class="row">
      <span class="count">{count}</span>
      <button type="button" onclick={() => call("increment", 1)}>+1</button>
      <button type="button" onclick={() => emit("reset")}>Reset</button>
    </div>
  </section>
  <footer>
    {uptime !== null ? `Fogu backend up for ${uptime}s` : inApp ? "Connecting to Fogu…" : "Not in the app: run fogu desktop dev"}
  </footer>
</main>
