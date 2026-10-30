// JSON: stringify and parse 50k records
const rows = [];
for (let i = 0; i < 50000; i++) rows.push({ id: i, name: "user" + i, active: i % 2 === 0, tags: ["a", "b"] });
const text = JSON.stringify(rows);
const back = JSON.parse(text);
console.log(back.length, back[49999].name, text.length);
