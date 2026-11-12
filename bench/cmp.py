"""Compares commands on a noisy machine: N interleaved runs of each, minimum
wall time / minimum CPU time (user + system) in ms. Trust the wall time.

    go build -o /tmp/fib-go bench/fib.go
    python3 bench/cmp.py 100 "new=target/release/fogu bench/fib.fg" \
        "old=/path/to/previous/fogu bench/fib.fg" "go=/tmp/fib-go"

A command may start with VAR=value settings."""
import os, sys, time, shlex, subprocess
n = int(sys.argv[1])
cmds = [a.split('=', 1) for a in sys.argv[2:]]
best = {l: [1e9, 1e9] for l, _ in cmds}
devnull = open(os.devnull, 'w')
for _ in range(n):
    for label, cmd in cmds:
        argv = shlex.split(cmd)
        env = dict(os.environ)
        while argv and '=' in argv[0] and not argv[0].startswith(('/', '.')):
            k, v = argv.pop(0).split('=', 1); env[k] = v
        t0 = time.perf_counter()
        p = subprocess.Popen(argv, stdout=devnull, stderr=devnull, env=env)
        _, status, ru = os.wait4(p.pid, 0)
        wall = (time.perf_counter() - t0) * 1000
        cpu = (ru.ru_utime + ru.ru_stime) * 1000
        b = best[label]
        b[0] = min(b[0], wall); b[1] = min(b[1], cpu)
print(' '.join(f"{l}: {best[l][0]:.1f}/{best[l][1]:.1f}" for l, _ in cmds))
