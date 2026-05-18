#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main() {
    int cap = 16;
    char *s = malloc(cap);
    s[0] = '\0';
    int len = 0;
    for (int i = 0; i < 10000; i++) {
        if (len + 1 >= cap) {
            cap *= 2;
            s = realloc(s, cap);
        }
        s[len++] = 'x';
        s[len] = '\0';
    }
    printf("%d\n", len);
    free(s);
    return 0;
}
