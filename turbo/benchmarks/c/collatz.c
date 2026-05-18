#include <stdio.h>

long collatz_len(long n) {
    long x = n, l = 0;
    while (x != 1) {
        if (x % 2 == 0) x = x / 2; else x = 3 * x + 1;
        l++;
    }
    return l;
}

int main() {
    long max_len = 0;
    for (long i = 1; i <= 1000000; i++) {
        long l = collatz_len(i);
        if (l > max_len) max_len = l;
    }
    printf("%ld\n", max_len);
    return 0;
}
