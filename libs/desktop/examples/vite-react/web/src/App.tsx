// The page: call() runs a Fogu command (main.fg), on() receives its events.
import { useEffect, useState, type FormEvent } from "react";
import { call, emit, on, isDesktop } from "./fogu-desktop.js";

type Greeting = { message: string };

/** A Fogu event as React state. */
function useFoguEvent<T>(name: string, initial: T): T {
  const [value, setValue] = useState<T>(initial);
  useEffect(() => on<T>(name, setValue), [name]);
  return value;
}

export default function App() {
  const [name, setName] = useState("");
  const [answer, setAnswer] = useState<{ text: string; error: boolean } | null>(null);
  const count = useFoguEvent<number>("count", 0);
  const uptime = useFoguEvent<number | null>("tick", null);

  // tell the backend which page loaded (the embedded build or the dev server)
  useEffect(() => {
    if (isDesktop()) {
      void emit("page-ready", { url: location.href });
    }
  }, []);

  async function greet(e: FormEvent) {
    e.preventDefault();
    try {
      const r = await call<Greeting>("greet", name);
      setAnswer({ text: r.message, error: false });
    } catch (err) {
      setAnswer({ text: (err as Error).message, error: true });
    }
  }

  return (
    <main>
      <h1>Fogu Vite React</h1>
      <p className="sub">Vite + React in front, a Fogu program behind.</p>
      <section>
        <h2>Command</h2>
        <form onSubmit={greet}>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Your name" autoComplete="off" />
          <button className="primary" type="submit">Greet</button>
        </form>
        <p className={answer?.error ? "out error" : "out"}>{answer?.text}</p>
      </section>
      <section>
        <h2>Events</h2>
        <div className="row">
          <span className="count">{count}</span>
          <button type="button" onClick={() => void call<number>("increment", 1)}>+1</button>
          <button type="button" onClick={() => void emit("reset")}>Reset</button>
        </div>
      </section>
      <footer>
        {uptime !== null
          ? `Fogu backend up for ${uptime}s`
          : isDesktop()
            ? "Connecting to Fogu…"
            : "Not in the app: run fogu desktop dev"}
      </footer>
    </main>
  );
}
