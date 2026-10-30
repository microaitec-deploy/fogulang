// sorting 300k pseudo-random integers
let x = 42;
const xs = [];
for (let i = 0; i < 300000; i++) {
  x = (Math.imul(x, 1103515245) + 12345) & 0x7fffffff; // the same value mod 2^31, without losing precision
  xs.push(x);
}
const s = Float64Array.from(xs).sort();
console.log(s[0], s[150000], s[299999]);
