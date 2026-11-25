<!-- The page: call() runs a Fogu command (main.fg), on() receives its events. -->
<script setup lang="ts">
import { onUnmounted, ref } from "vue";
import { call, emit, on, isDesktop } from "./fogu-desktop.js";

type Greeting = { message: string };

const name = ref("");
const answer = ref<{ text: string; error: boolean } | null>(null);
const count = ref(0);
const uptime = ref<number | null>(null);
const inApp = isDesktop();

const offs = [on<number>("count", (n) => (count.value = n)), on<number>("tick", (s) => (uptime.value = s))];
onUnmounted(() => offs.forEach((off) => off()));

async function greet() {
  try {
    const r = await call<Greeting>("greet", name.value);
    answer.value = { text: r.message, error: false };
  } catch (err) {
    answer.value = { text: (err as Error).message, error: true };
  }
}
</script>

<template>
  <main>
    <h1>{{title}}</h1>
    <p class="sub">Vite + Vue in front, a Fogu program behind.</p>
    <section>
      <h2>Command</h2>
      <form @submit.prevent="greet">
        <input v-model="name" placeholder="Your name" autocomplete="off" />
        <button class="primary" type="submit">Greet</button>
      </form>
      <p :class="answer?.error ? 'out error' : 'out'">{{ answer?.text }}</p>
    </section>
    <section>
      <h2>Events</h2>
      <div class="row">
        <span class="count">{{ count }}</span>
        <button type="button" @click="call('increment', 1)">+1</button>
        <button type="button" @click="emit('reset')">Reset</button>
      </div>
    </section>
    <footer>
      {{ uptime !== null ? `Fogu backend up for ${uptime}s` : inApp ? "Connecting to Fogu…" : "Not in the app: run fogu desktop dev" }}
    </footer>
  </main>
</template>
