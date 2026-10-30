// maps: insert 300k keys, then read them back
const m = new Map();
for (let i = 0; i < 300000; i++) m.set("k" + i, i);
let sum = 0;
for (let i = 0; i < 300000; i++) sum += m.get("k" + i);
console.log(m.size, sum);
