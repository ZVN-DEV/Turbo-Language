#include <stdio.h>

int main() {
    double sum = 0.0;
    double sign = 1.0;
    for (int i = 0; i < 50000000; i++) {
        sum += sign / (2 * i + 1);
        sign = -sign;
    }
    printf("%.16f\n", sum * 4.0);
    return 0;
}
