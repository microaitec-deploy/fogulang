// A program, not a kernel: write a 100k-line text file, read it back, count
// the words and print the ten most frequent.
package main

import (
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"
)

var seed = 11

func rnd(n int) int {
	seed = (seed*75 + 74) % 65537
	return seed % n
}

var syl = []string{"ka", "to", "mi", "re", "su", "no", "la", "pe", "di", "vu"}
var vocab []string

func makeText(lines int) string {
	out := make([]string, 0, lines)
	for l := 0; l < lines; l++ {
		words := make([]string, 0, 10)
		for w := 0; w < 10; w++ {
			a := rnd(2000)
			b := rnd(2000)
			word := vocab[a*b/2000]
			if w == 0 {
				word = strings.ToUpper(word)
			}
			words = append(words, word)
		}
		out = append(out, strings.Join(words, " ")+".")
	}
	return strings.Join(out, "\n") + "\n"
}

func count(text string) map[string]int {
	counts := map[string]int{}
	for _, line := range strings.Split(text, "\n") {
		if len(line) == 0 {
			continue
		}
		for _, w := range strings.Split(strings.ReplaceAll(strings.ToLower(line), ".", ""), " ") {
			counts[w]++
		}
	}
	return counts
}

func report(counts map[string]int) {
	top := make([]string, 0, len(counts))
	for k := range counts {
		top = append(top, k)
	}
	sort.Slice(top, func(i, j int) bool {
		a, b := top[i], top[j]
		if counts[a] != counts[b] {
			return counts[a] > counts[b]
		}
		return a < b
	})
	total := 0
	for _, k := range top {
		total += counts[k]
	}
	line := ""
	for i := 0; i < 10; i++ {
		line += top[i] + "=" + strconv.Itoa(counts[top[i]]) + " "
	}
	fmt.Println(strings.TrimSpace(line))
	fmt.Println(len(top), total)
}

func main() {
	for i := 0; i < 2000; i++ {
		vocab = append(vocab, syl[i%10]+syl[(i/10)%10]+syl[(i/100)%10]+strconv.Itoa(i/1000))
	}
	path := "/tmp/fogu_bench_wordfreq_go.txt"
	if err := os.WriteFile(path, []byte(makeText(100000)), 0o644); err != nil {
		panic(err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		panic(err)
	}
	report(count(string(data)))
	os.Remove(path)
}
