// an integer loop: 20 million iterations
package main

import "fmt"

func main() {
	sum := 0
	for i := 0; i < 20000000; i++ {
		sum = sum + (i%7)*3
	}
	fmt.Println(sum)
}
