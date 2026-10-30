// an integer loop: 20 million iterations
let sum = 0;
for (let i = 0; i < 20000000; i++) sum = sum + (i % 7) * 3;
console.log(sum);
