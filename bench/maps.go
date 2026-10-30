// maps: insert 300k keys, then read them back
package main

import (
	"fmt"
	"strconv"
)

func main() {
	m := map[string]int{}
	for i := 0; i < 300000; i++ {
		m["k"+strconv.Itoa(i)] = i
	}
	sum := 0
	for i := 0; i < 300000; i++ {
		sum += m["k"+strconv.Itoa(i)]
	}
	fmt.Println(len(m), sum)
}
