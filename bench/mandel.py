# floating point: points of a 300x300 Mandelbrot grid (50 iterations)
inside = 0
for y in range(300):
    for x in range(300):
        cr = x * 3.0 / 300 - 2.0
        ci = y * 2.0 / 300 - 1.0
        zr = zi = 0.0
        n = 0
        while n < 50 and zr * zr + zi * zi <= 4.0:
            t = zr * zr - zi * zi + cr
            zi = 2.0 * zr * zi + ci
            zr = t
            n += 1
        if n == 50:
            inside += 1
print(inside)
