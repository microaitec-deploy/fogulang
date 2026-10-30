# an integer loop: 20 million iterations
s = 0
for i in range(20000000):
    s = s + (i % 7) * 3
print(s)
