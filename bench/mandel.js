// floating point: points of a 300x300 Mandelbrot grid (50 iterations)
let inside = 0;
for (let y = 0; y < 300; y++) {
  for (let x = 0; x < 300; x++) {
    const cr = (x * 3.0) / 300 - 2.0;
    const ci = (y * 2.0) / 300 - 1.0;
    let zr = 0.0, zi = 0.0, n = 0;
    while (n < 50 && zr * zr + zi * zi <= 4.0) {
      const t = zr * zr - zi * zi + cr;
      zi = 2.0 * zr * zi + ci;
      zr = t;
      n++;
    }
    if (n === 50) inside++;
  }
}
console.log(inside);
