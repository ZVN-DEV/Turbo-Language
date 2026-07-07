/* CI smoke baseline: recursive Fibonacci, identical algorithm to fib.tb.
 * Compiled with `cc -O2`. Must print byte-identical output to the Turbo build. */
#include <stdio.h>
long long fib(int n) { if (n <= 1) return n; return fib(n - 1) + fib(n - 2); }
int main(void) { printf("%lld\n", fib(35)); return 0; }
