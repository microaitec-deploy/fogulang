# A program, not a kernel: a limit order book. 200k orders are matched against
# two binary heaps of objects (best bid, best ask).
M = 1000003
seed = 7


def rnd(n):
    global seed
    seed = (seed * 75 + 74) % 65537
    return seed % n


class Order:
    def __init__(self, id, price, qty):
        self.id = id
        self.price = price
        self.qty = qty


# a binary heap: sign 1 keeps the lowest price on top, -1 the highest
class Heap:
    def __init__(self, sign):
        self.items = []
        self.sign = sign

    def less(self, a, b):
        pa = a.price * self.sign
        pb = b.price * self.sign
        if pa != pb:
            return pa < pb
        return a.id < b.id

    def size(self):
        return len(self.items)

    def top(self):
        return self.items[0]

    def push(self, o):
        xs = self.items
        xs.append(o)
        i = len(xs) - 1
        while i > 0:
            parent = (i - 1) // 2
            if not self.less(xs[i], xs[parent]):
                break
            xs[i], xs[parent] = xs[parent], xs[i]
            i = parent

    def pop(self):
        xs = self.items
        last = xs.pop()
        n = len(xs)
        if n == 0:
            return
        xs[0] = last
        i = 0
        while True:
            l = 2 * i + 1
            r = l + 1
            best = i
            if l < n and self.less(xs[l], xs[best]):
                best = l
            if r < n and self.less(xs[r], xs[best]):
                best = r
            if best == i:
                break
            xs[i], xs[best] = xs[best], xs[i]
            i = best


bids = Heap(-1)
asks = Heap(1)
trades = 0
volume = 0


def submit(id):
    global trades, volume
    buy = rnd(2) == 0
    price = 900 + rnd(200)
    qty = 1 + rnd(50)
    if buy:
        while qty > 0 and asks.size() > 0 and asks.top().price <= price:
            best = asks.top()
            q = min(qty, best.qty)
            trades += 1
            volume = (volume + q * best.price) % M
            qty -= q
            best.qty -= q
            if best.qty == 0:
                asks.pop()
        if qty > 0:
            bids.push(Order(id, price, qty))
    else:
        while qty > 0 and bids.size() > 0 and bids.top().price >= price:
            best = bids.top()
            q = min(qty, best.qty)
            trades += 1
            volume = (volume + q * best.price) % M
            qty -= q
            best.qty -= q
            if best.qty == 0:
                bids.pop()
        if qty > 0:
            asks.push(Order(id, price, qty))


for i in range(200000):
    submit(i)
print(trades, volume, bids.size(), asks.size())
