// A program, not a kernel: generate 20k arithmetic expressions, tokenize them,
// parse them into trees (recursive descent) and evaluate them.
package main

import (
	"fmt"
	"strconv"
)

const M = 1000003

var seed = 42

func rnd(n int) int {
	seed = (seed*75 + 74) % 65537
	return seed % n
}

var names = []string{"a", "b", "c", "d"}

func gen(depth int) string {
	if depth == 0 || rnd(4) == 0 {
		if rnd(3) == 0 {
			return names[rnd(4)]
		}
		return strconv.Itoa(rnd(100))
	}
	op := []string{"+", "*"}[rnd(2)]
	left := gen(depth - 1)
	right := gen(depth - 1)
	return "(" + left + " " + op + " " + right + ")"
}

type token struct {
	kind  string
	value int
	text  string
}

func tokenize(src string) []token {
	toks := []token{}
	n := len(src)
	i := 0
	for i < n {
		c := src[i]
		if c == ' ' {
			i++
		} else if c >= '0' && c <= '9' {
			v := 0
			for i < n && src[i] >= '0' && src[i] <= '9' {
				v = v*10 + int(src[i]-'0')
				i++
			}
			toks = append(toks, token{"num", v, ""})
		} else if c >= 'a' && c <= 'z' {
			toks = append(toks, token{"id", 0, src[i : i+1]})
			i++
		} else {
			toks = append(toks, token{"op", 0, src[i : i+1]})
			i++
		}
	}
	return toks
}

type node struct {
	op    string
	value int
	name  string
	l, r  *node
}

type parser struct {
	toks []token
	pos  int
}

func (p *parser) peek() string {
	if p.pos < len(p.toks) {
		return p.toks[p.pos].text
	}
	return ""
}

func (p *parser) expr() *node {
	left := p.term()
	for p.peek() == "+" {
		p.pos++
		left = &node{op: "+", l: left, r: p.term()}
	}
	return left
}

func (p *parser) term() *node {
	left := p.factor()
	for p.peek() == "*" {
		p.pos++
		left = &node{op: "*", l: left, r: p.factor()}
	}
	return left
}

func (p *parser) factor() *node {
	t := p.toks[p.pos]
	p.pos++
	if t.kind == "num" {
		return &node{op: "num", value: t.value}
	}
	if t.kind == "id" {
		return &node{op: "id", name: t.text}
	}
	e := p.expr()
	p.pos++ // ")"
	return e
}

var env = map[string]int{"a": 3, "b": 5, "c": 7, "d": 11}

func eval(n *node) int {
	switch n.op {
	case "num":
		return n.value
	case "id":
		return env[n.name]
	}
	l := eval(n.l)
	r := eval(n.r)
	if n.op == "+" {
		return (l + r) % M
	}
	return (l * r) % M
}

func main() {
	sum := 0
	tokens := 0
	for i := 0; i < 20000; i++ {
		src := gen(6)
		toks := tokenize(src)
		tokens += len(toks)
		p := &parser{toks: toks}
		sum = (sum + eval(p.expr())) % M
	}
	fmt.Println(sum, tokens)
}
