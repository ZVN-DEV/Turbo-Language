#include <stdio.h>

int main() {
    int arr[200];
    for (int i = 0; i < 200; i++) arr[i] = 200 - i;
    int n = 200;
    for (int i = 0; i < n; i++) {
        for (int j = 0; j < n - 1; j++) {
            if (arr[j] > arr[j + 1]) {
                int tmp = arr[j];
                arr[j] = arr[j + 1];
                arr[j + 1] = tmp;
            }
        }
    }
    printf("%d\n%d\n", arr[0], arr[199]);
    return 0;
}
