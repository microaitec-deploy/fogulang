# JSON: stringify and parse 50k records
import json
rows = [{"id": i, "name": "user" + str(i), "active": i % 2 == 0, "tags": ["a", "b"]} for i in range(50000)]
text = json.dumps(rows, separators=(",", ":"))
back = json.loads(text)
print(len(back), back[49999]["name"], len(text))
