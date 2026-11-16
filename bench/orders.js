// A program, not a kernel: a limit order book. 200k orders are matched against
// two binary heaps of objects (best bid, best ask).
const M = 1000003
let seed = 7
function rnd(n) {
    seed = (seed * 75 + 74) % 65537
    return seed % n
}

class Order {
    constructor(id, price, qty) {
        this.id = id
        this.price = price
        this.qty = qty
    }
}

// a binary heap: sign 1 keeps the lowest price on top, -1 the highest
class Heap {
    constructor(sign) {
        this.items = []
        this.sign = sign
    }
    less(a, b) {
        const pa = a.price * this.sign
        const pb = b.price * this.sign
        if (pa !== pb) return pa < pb
        return a.id < b.id
    }
    size() { return this.items.length }
    top() { return this.items[0] }
    push(o) {
        const xs = this.items
        xs.push(o)
        let i = xs.length - 1
        while (i > 0) {
            const parent = (i - 1) >> 1
            if (!this.less(xs[i], xs[parent])) break
            const t = xs[i]
            xs[i] = xs[parent]
            xs[parent] = t
            i = parent
        }
    }
    pop() {
        const xs = this.items
        const last = xs.pop()
        const n = xs.length
        if (n === 0) return
        xs[0] = last
        let i = 0
        for (;;) {
            const l = 2 * i + 1
            const r = l + 1
            let best = i
            if (l < n && this.less(xs[l], xs[best])) best = l
            if (r < n && this.less(xs[r], xs[best])) best = r
            if (best === i) break
            const t = xs[i]
            xs[i] = xs[best]
            xs[best] = t
            i = best
        }
    }
}

const bids = new Heap(-1)
const asks = new Heap(1)
let trades = 0
let volume = 0

function submit(id) {
    const buy = rnd(2) === 0
    const price = 900 + rnd(200)
    let qty = 1 + rnd(50)
    const own = buy ? bids : asks
    const other = buy ? asks : bids
    while (qty > 0 && other.size() > 0 && (buy ? other.top().price <= price : other.top().price >= price)) {
        const best = other.top()
        const q = Math.min(qty, best.qty)
        trades++
        volume = (volume + q * best.price) % M
        qty -= q
        best.qty -= q
        if (best.qty === 0) other.pop()
    }
    if (qty > 0) own.push(new Order(id, price, qty))
}

for (let i = 0; i < 200000; i++) submit(i)
console.log(trades, volume, bids.size(), asks.size())
