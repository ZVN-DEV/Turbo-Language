"""Reference: Longest Collatz sequence in Python"""

def collatz_len(n):
    x = n
    l = 0
    while x != 1:
        if x % 2 == 0:
            x = x // 2
        else:
            x = 3 * x + 1
        l += 1
    return l

max_len = 0
for i in range(1, 1000001):
    l = collatz_len(i)
    if l > max_len:
        max_len = l
print(max_len)
