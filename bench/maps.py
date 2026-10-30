# maps: insert 300k keys, then read them back
m = {}
for i in range(300000):
    m["k" + str(i)] = i
s = 0
for i in range(300000):
    s += m["k" + str(i)]
print(len(m), s)
