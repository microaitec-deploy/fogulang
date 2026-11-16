// A program, not a kernel: write a 100k-line text file, read it back, count
// the words and print the ten most frequent.
const fs = require("fs")

let seed = 11
function rnd(n) {
    seed = (seed * 75 + 74) % 65537
    return seed % n
}

const syl = ["ka", "to", "mi", "re", "su", "no", "la", "pe", "di", "vu"]
const vocab = []
for (let i = 0; i < 2000; i++) {
    vocab.push(syl[i % 10] + syl[Math.floor(i / 10) % 10] + syl[Math.floor(i / 100) % 10] + String(Math.floor(i / 1000)))
}

function makeText(lines) {
    const out = []
    for (let l = 0; l < lines; l++) {
        const words = []
        for (let w = 0; w < 10; w++) {
            const a = rnd(2000)
            const b = rnd(2000)
            const word = vocab[Math.floor(a * b / 2000)]
            words.push(w === 0 ? word.toUpperCase() : word)
        }
        out.push(words.join(" ") + ".")
    }
    return out.join("\n") + "\n"
}

function count(text) {
    const counts = new Map()
    for (const line of text.split("\n")) {
        if (line.length === 0) continue
        for (const w of line.toLowerCase().replaceAll(".", "").split(" ")) {
            counts.set(w, (counts.get(w) ?? 0) + 1)
        }
    }
    return counts
}

function report(counts) {
    const top = [...counts.keys()].sort((a, b) => {
        if (counts.get(a) !== counts.get(b)) return counts.get(b) - counts.get(a)
        return a < b ? -1 : 1
    })
    let total = 0
    for (const k of top) total += counts.get(k)
    let line = ""
    for (let i = 0; i < 10; i++) line += top[i] + "=" + counts.get(top[i]) + " "
    console.log(line.trim())
    console.log(top.length, total)
}

const path = "/tmp/fogu_bench_wordfreq_js.txt"
fs.writeFileSync(path, makeText(100000))
const text = fs.readFileSync(path, "utf8")
report(count(text))
fs.unlinkSync(path)
