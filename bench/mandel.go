// floating point: points of a 300x300 Mandelbrot grid (50 iterations)
package main

import "fmt"

func main() {
	inside := 0
	for y := 0; y < 300; y++ {
		for x := 0; x < 300; x++ {
			cr := float64(x)*3.0/300 - 2.0
			ci := float64(y)*2.0/300 - 1.0
			zr, zi, n := 0.0, 0.0, 0
			for n < 50 && zr*zr+zi*zi <= 4.0 {
				t := zr*zr - zi*zi + cr
				zi = 2.0*zr*zi + ci
				zr = t
				n++
			}
			if n == 50 {
				inside++
			}
		}
	}
	fmt.Println(inside)
}
