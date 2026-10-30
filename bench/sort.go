// sorting 300k pseudo-random integers
package main

import (
	"fmt"
	"sort"
)

func main() {
	x := int64(42)
	xs := make([]int64, 0, 300000)
	for i := 0; i < 300000; i++ {
		x = (x*1103515245 + 12345) % 2147483648
		xs = append(xs, x)
	}
	sort.Slice(xs, func(a, b int) bool { return xs[a] < xs[b] })
	fmt.Println(xs[0], xs[150000], xs[299999])
}
