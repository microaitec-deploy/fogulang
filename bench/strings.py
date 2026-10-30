# strings: build 200k items, join, split and measure
parts = []
for i in range(200000):
    parts.append("item" + str(i))
text = ",".join(parts)
back = text.split(",")
total = 0
for p in back:
    total += len(p)
print(len(back), total, len(text))
