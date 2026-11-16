// A program, not a kernel: a limit order book. 200k orders are matched against
// two binary heaps of structs (best bid, best ask).
package main

import "fmt"

const M = 1000003

var seed = 7

func rnd(n int) int {
	seed = (seed*75 + 74) % 65537
	return seed % n
}

type order struct {
	id, price, qty int
}

// a binary heap: sign 1 keeps the lowest price on top, -1 the highest
type heap struct {
	items []*order
	sign  int
}

func (h *heap) less(a, b *order) bool {
	pa := a.price * h.sign
	pb := b.price * h.sign
	if pa != pb {
		return pa < pb
	}
	return a.id < b.id
}

func (h *heap) size() int   { return len(h.items) }
func (h *heap) top() *order { return h.items[0] }

func (h *heap) push(o *order) {
	h.items = append(h.items, o)
	xs := h.items
	i := len(xs) - 1
	for i > 0 {
		parent := (i - 1) / 2
		if !h.less(xs[i], xs[parent]) {
			break
		}
		xs[i], xs[parent] = xs[parent], xs[i]
		i = parent
	}
}

func (h *heap) pop() {
	last := h.items[len(h.items)-1]
	h.items = h.items[:len(h.items)-1]
	xs := h.items
	n := len(xs)
	if n == 0 {
		return
	}
	xs[0] = last
	i := 0
	for {
		l := 2*i + 1
		r := l + 1
		best := i
		if l < n && h.less(xs[l], xs[best]) {
			best = l
		}
		if r < n && h.less(xs[r], xs[best]) {
			best = r
		}
		if best == i {
			break
		}
		xs[i], xs[best] = xs[best], xs[i]
		i = best
	}
}

var bids = &heap{sign: -1}
var asks = &heap{sign: 1}
var trades, volume int

func submit(id int) {
	buy := rnd(2) == 0
	price := 900 + rnd(200)
	qty := 1 + rnd(50)
	if buy {
		for qty > 0 && asks.size() > 0 && asks.top().price <= price {
			best := asks.top()
			q := qty
			if best.qty < q {
				q = best.qty
			}
			trades++
			volume = (volume + q*best.price) % M
			qty -= q
			best.qty -= q
			if best.qty == 0 {
				asks.pop()
			}
		}
		if qty > 0 {
			bids.push(&order{id, price, qty})
		}
	} else {
		for qty > 0 && bids.size() > 0 && bids.top().price >= price {
			best := bids.top()
			q := qty
			if best.qty < q {
				q = best.qty
			}
			trades++
			volume = (volume + q*best.price) % M
			qty -= q
			best.qty -= q
			if best.qty == 0 {
				bids.pop()
			}
		}
		if qty > 0 {
			asks.push(&order{id, price, qty})
		}
	}
}

func main() {
	for i := 0; i < 200000; i++ {
		submit(i)
	}
	fmt.Println(trades, volume, bids.size(), asks.size())
}
