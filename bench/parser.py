# A program, not a kernel: generate 20k arithmetic expressions, tokenize them,
# parse them into trees (recursive descent) and evaluate them.
import sys

sys.setrecursionlimit(10000)
M = 1000003
seed = 42


def rnd(n):
    global seed
    seed = (seed * 75 + 74) % 65537
    return seed % n


names = ["a", "b", "c", "d"]


def gen(depth):
    if depth == 0 or rnd(4) == 0:
        if rnd(3) == 0:
            return names[rnd(4)]
        return str(rnd(100))
    op = ["+", "*"][rnd(2)]
    left = gen(depth - 1)
    right = gen(depth - 1)
    return "(" + left + " " + op + " " + right + ")"


def tokenize(src):
    toks = []
    n = len(src)
    i = 0
    while i < n:
        c = src[i]
        if c == " ":
            i += 1
        elif "0" <= c <= "9":
            v = 0
            while i < n and "0" <= src[i] <= "9":
                v = v * 10 + ord(src[i]) - 48
                i += 1
            toks.append({"kind": "num", "value": v, "text": ""})
        elif "a" <= c <= "z":
            toks.append({"kind": "id", "value": 0, "text": c})
            i += 1
        else:
            toks.append({"kind": "op", "value": 0, "text": c})
            i += 1
    return toks


class Parser:
    def __init__(self, toks):
        self.toks = toks
        self.pos = 0

    def peek(self):
        if self.pos < len(self.toks):
            return self.toks[self.pos]["text"]
        return ""

    def expr(self):
        left = self.term()
        while self.peek() == "+":
            self.pos += 1
            left = {"op": "+", "l": left, "r": self.term()}
        return left

    def term(self):
        left = self.factor()
        while self.peek() == "*":
            self.pos += 1
            left = {"op": "*", "l": left, "r": self.factor()}
        return left

    def factor(self):
        t = self.toks[self.pos]
        self.pos += 1
        if t["kind"] == "num":
            return {"op": "num", "value": t["value"]}
        if t["kind"] == "id":
            return {"op": "id", "name": t["text"]}
        e = self.expr()
        self.pos += 1  # ")"
        return e


env = {"a": 3, "b": 5, "c": 7, "d": 11}


def evaluate(node):
    op = node["op"]
    if op == "num":
        return node["value"]
    if op == "id":
        return env[node["name"]]
    l = evaluate(node["l"])
    r = evaluate(node["r"])
    if op == "+":
        return (l + r) % M
    return (l * r) % M


total = 0
tokens = 0
for i in range(20000):
    src = gen(6)
    toks = tokenize(src)
    tokens += len(toks)
    p = Parser(toks)
    total = (total + evaluate(p.expr())) % M
print(total, tokens)
