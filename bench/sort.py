# sorting 300k pseudo-random integers
x = 42
xs = []
for i in range(300000):
    x = (x * 1103515245 + 12345) % 2147483648
    xs.append(x)
s = sorted(xs)
print(s[0], s[150000], s[299999])
