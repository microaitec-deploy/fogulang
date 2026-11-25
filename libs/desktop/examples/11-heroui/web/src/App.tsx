// The page: React with HeroUI components and Tailwind CSS classes. call() runs
// a Fogu command (main.fg), on() receives its events, emit() sends one.
import { useEffect, useState, type FormEvent } from "react";
import {
  Button,
  Card,
  Checkbox,
  Chip,
  Input,
  Label,
  ProgressBar,
  Switch,
  TextField,
  Toast,
  toast,
} from "@heroui/react";
import { call, emit, on, isDesktop } from "./fogu-desktop.js";

type Tag = "work" | "home" | "study" | "other";
type Task = { id: number; title: string; tag: Tag; done: boolean };
type Summary = { items: Task[]; done: number; total: number; percent: number };

const TAGS: Tag[] = ["work", "home", "study", "other"];
const TAG_COLOR = { work: "accent", home: "success", study: "warning", other: "default" } as const;

/** Runs a Fogu command; its error value becomes a toast. */
async function run<T>(name: string, payload?: unknown): Promise<T | undefined> {
  try {
    return await call<T>(name, payload);
  } catch (err) {
    toast.danger((err as Error).message);
    return undefined;
  }
}

export default function App() {
  const [tasks, setTasks] = useState<Summary>({ items: [], done: 0, total: 0, percent: 0 });
  const [title, setTitle] = useState("");
  const [tag, setTag] = useState<Tag>("work");
  const [dark, setDark] = useState(() => matchMedia("(prefers-color-scheme: dark)").matches);

  // the list now, then every change Fogu pushes
  useEffect(() => {
    if (!isDesktop()) return;
    void run<Summary>("tasks.list").then((s) => s && setTasks(s));
    return on<Summary>("tasks", setTasks);
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);

  async function add(e: FormEvent) {
    e.preventDefault();
    const task = await run<Task>("tasks.add", { title, tag });
    if (task) {
      setTitle("");
      toast.success(`Added “${task.title}”`);
    }
  }

  return (
    <main className="min-h-full bg-background text-foreground">
      <div className="mx-auto flex max-w-2xl flex-col gap-6 px-6 py-8">
        <header className="flex items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold tracking-tight">Fogu Tasks</h1>
            <p className="text-sm text-muted">React, HeroUI and Tailwind CSS in front; a Fogu program behind.</p>
          </div>
          <Switch isSelected={dark} onChange={setDark}>
            <Switch.Content>
              <Switch.Control>
                <Switch.Thumb />
              </Switch.Control>
              <Label>Dark</Label>
            </Switch.Content>
          </Switch>
        </header>

        <Card>
          <Card.Header>
            <Card.Title>New task</Card.Title>
            <Card.Description>Fogu checks it and saves it in the app's data folder.</Card.Description>
          </Card.Header>
          <Card.Content>
            <form onSubmit={add} className="flex flex-col gap-4">
              <TextField value={title} onChange={setTitle} fullWidth>
                <Label>What needs to be done?</Label>
                <Input placeholder="Write the release notes" autoComplete="off" />
              </TextField>
              <div className="flex flex-wrap items-center gap-2">
                {TAGS.map((t) => (
                  <Button key={t} size="sm" variant={t === tag ? "primary" : "secondary"} onPress={() => setTag(t)}>
                    {t}
                  </Button>
                ))}
                <Button type="submit" className="ml-auto" isDisabled={!isDesktop()}>
                  Add
                </Button>
              </div>
            </form>
          </Card.Content>
        </Card>

        <Card>
          <Card.Header>
            <Card.Title>Tasks</Card.Title>
            <Card.Description>
              {tasks.total === 0 ? "Nothing yet." : `${tasks.done} of ${tasks.total} done`}
            </Card.Description>
          </Card.Header>
          <Card.Content className="flex flex-col gap-4">
            <ProgressBar value={tasks.percent} aria-label="Done">
              <ProgressBar.Track>
                <ProgressBar.Fill />
              </ProgressBar.Track>
            </ProgressBar>
            <ul className="flex flex-col divide-y divide-separator">
              {tasks.items.map((t) => (
                <li key={t.id} className="flex items-center gap-3 py-2">
                  <Checkbox isSelected={t.done} onChange={() => void run("tasks.toggle", t.id)}>
                    <Checkbox.Content>
                      <Checkbox.Control>
                        <Checkbox.Indicator />
                      </Checkbox.Control>
                      <Label className={t.done ? "text-muted line-through" : ""}>{t.title}</Label>
                    </Checkbox.Content>
                  </Checkbox>
                  <Chip size="sm" color={TAG_COLOR[t.tag]} className="ml-auto">
                    {t.tag}
                  </Chip>
                  <Button size="sm" variant="ghost" onPress={() => void run("tasks.remove", t.id)}>
                    Remove
                  </Button>
                </li>
              ))}
            </ul>
          </Card.Content>
          <Card.Footer className="justify-end">
            <Button variant="danger-soft" isDisabled={tasks.done === 0} onPress={() => void emit("tasks.clear-done")}>
              Clear done
            </Button>
          </Card.Footer>
        </Card>

        {!isDesktop() && <p className="text-center text-sm text-muted">Not in the app: run fogu desktop dev</p>}
      </div>
      <Toast.Provider placement="bottom end" />
    </main>
  );
}
