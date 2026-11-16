// A program, not a kernel: generate 20k arithmetic expressions, tokenize them,
// parse them into trees (recursive descent) and evaluate them.
const M = 1000003
let seed = 42
function rnd(n) {
    seed = (seed * 75 + 74) % 65537
    return seed % n
}

const names = ["a", "b", "c", "d"]
function gen(depth) {
    if (depth === 0 || rnd(4) === 0) {
        if (rnd(3) === 0) return names[rnd(4)]
        return String(rnd(100))
    }
    const op = ["+", "*"][rnd(2)]
    const left = gen(depth - 1)
    const right = gen(depth - 1)
    return "(" + left + " " + op + " " + right + ")"
}

function tokenize(src) {
    const toks = []
    const n = src.length
    let i = 0
    while (i < n) {
        const k = src.charCodeAt(i)
        if (k === 32) {
            i++
        } else if (k >= 48 && k <= 57) {
            let v = 0
            while (i < n && src.charCodeAt(i) >= 48 && src.charCodeAt(i) <= 57) {
                v = v * 10 + src.charCodeAt(i) - 48
                i++
            }
            toks.push({ kind: "num", value: v, text: "" })
        } else if (k >= 97 && k <= 122) {
            toks.push({ kind: "id", value: 0, text: src[i] })
            i++
        } else {
            toks.push({ kind: "op", value: 0, text: src[i] })
            i++
        }
    }
    return toks
}

class Parser {
    constructor(toks) {
        this.toks = toks
        this.pos = 0
    }
    peek() {
        if (this.pos < this.toks.length) return this.toks[this.pos].text
        return ""
    }
    expr() {
        let left = this.term()
        while (this.peek() === "+") {
            this.pos++
            left = { op: "+", l: left, r: this.term() }
        }
        return left
    }
    term() {
        let left = this.factor()
        while (this.peek() === "*") {
            this.pos++
            left = { op: "*", l: left, r: this.factor() }
        }
        return left
    }
    factor() {
        const t = this.toks[this.pos]
        this.pos++
        if (t.kind === "num") return { op: "num", value: t.value }
        if (t.kind === "id") return { op: "id", name: t.text }
        const e = this.expr()
        this.pos++ // ")"
        return e
    }
}

const env = { a: 3, b: 5, c: 7, d: 11 }
function evaluate(node) {
    const op = node.op
    if (op === "num") return node.value
    if (op === "id") return env[node.name]
    const l = evaluate(node.l)
    const r = evaluate(node.r)
    if (op === "+") return (l + r) % M
    return (l * r) % M
}

let sum = 0
let tokens = 0
for (let i = 0; i < 20000; i++) {
    const src = gen(6)
    const toks = tokenize(src)
    tokens += toks.length
    const p = new Parser(toks)
    sum = (sum + evaluate(p.expr())) % M
}
console.log(sum, tokens)
