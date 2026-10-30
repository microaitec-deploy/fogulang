// strings: build 200k items, join, split and measure
const parts = [];
for (let i = 0; i < 200000; i++) parts.push("item" + String(i));
const text = parts.join(",");
const back = text.split(",");
let total = 0;
for (const p of back) total += p.length;
console.log(back.length, total, text.length);
