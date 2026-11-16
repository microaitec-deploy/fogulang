# A program, not a kernel: write a 100k-line text file, read it back, count
# the words and print the ten most frequent.
import os

seed = 11


def rnd(n):
    global seed
    seed = (seed * 75 + 74) % 65537
    return seed % n


syl = ["ka", "to", "mi", "re", "su", "no", "la", "pe", "di", "vu"]
vocab = [syl[i % 10] + syl[(i // 10) % 10] + syl[(i // 100) % 10] + str(i // 1000) for i in range(2000)]


def make_text(lines):
    out = []
    for _ in range(lines):
        words = []
        for w in range(10):
            a = rnd(2000)
            b = rnd(2000)
            word = vocab[a * b // 2000]
            words.append(word.upper() if w == 0 else word)
        out.append(" ".join(words) + ".")
    return "\n".join(out) + "\n"


def count(text):
    counts = {}
    for line in text.split("\n"):
        if len(line) == 0:
            continue
        for w in line.lower().replace(".", "").split(" "):
            counts[w] = counts.get(w, 0) + 1
    return counts


def report(counts):
    top = sorted(counts, key=lambda k: (-counts[k], k))
    total = sum(counts[k] for k in top)
    print(" ".join(top[i] + "=" + str(counts[top[i]]) for i in range(10)))
    print(len(top), total)


path = "/tmp/fogu_bench_wordfreq_py.txt"
with open(path, "w") as f:
    f.write(make_text(100000))
with open(path) as f:
    text = f.read()
report(count(text))
os.remove(path)
