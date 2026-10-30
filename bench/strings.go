// strings: build 200k items, join, split and measure
package main

import (
	"fmt"
	"strconv"
	"strings"
	"unicode/utf8"
)

func main() {
	parts := []string{}
	for i := 0; i < 200000; i++ {
		parts = append(parts, "item"+strconv.Itoa(i))
	}
	text := strings.Join(parts, ",")
	back := strings.Split(text, ",")
	total := 0
	for _, p := range back {
		total += utf8.RuneCountInString(p)
	}
	fmt.Println(len(back), total, utf8.RuneCountInString(text))
}
