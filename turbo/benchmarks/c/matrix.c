#include <stdio.h>
#include <stdlib.h>

int main() {
    int n = 150;
    long *a = malloc(n * n * sizeof(long));
    long *b = malloc(n * n * sizeof(long));
    long *c = calloc(n * n, sizeof(long));
    for (int i = 0; i < n * n; i++) {
        a[i] = i % 100;
        b[i] = (i * 3) % 100;
    }
    for (int row = 0; row < n; row++) {
        for (int col = 0; col < n; col++) {
            long sum = 0;
            for (int k = 0; k < n; k++) {
                sum += a[row * n + k] * b[k * n + col];
            }
            c[row * n + col] = sum;
        }
    }
    long total = 0;
    for (int i = 0; i < n * n; i++) total += c[i];
    printf("%ld\n", total);
    free(a); free(b); free(c);
    return 0;
}
