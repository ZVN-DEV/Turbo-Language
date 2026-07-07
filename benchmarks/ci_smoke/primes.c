/* CI smoke baseline: prime counting via trial division, identical algorithm to
 * primes.tb. Compiled with `cc -O2`. Must print byte-identical output. */
#include <stdio.h>
#include <stdbool.h>

bool is_prime(long n) {
    if (n < 2) return false;
    for (long i = 2; i * i <= n; i++) {
        if (n % i == 0) return false;
    }
    return true;
}

int main(void) {
    int count = 0;
    for (long n = 2; n < 100000; n++) {
        if (is_prime(n)) count++;
    }
    printf("%d\n", count);
    return 0;
}
