#include <stdio.h>
#include <stdlib.h>
#include <string.h>

char **split(const char *s, char delim, int *count) {
    int n = 1;
    for (const char *p = s; *p; p++) if (*p == delim) n++;
    char **parts = malloc(n * sizeof(char *));
    const char *start = s;
    int idx = 0;
    for (const char *p = s; ; p++) {
        if (*p == delim || *p == '\0') {
            int len = (int)(p - start);
            parts[idx] = malloc(len + 1);
            memcpy(parts[idx], start, len);
            parts[idx][len] = '\0';
            idx++;
            if (*p == '\0') break;
            start = p + 1;
        }
    }
    *count = n;
    return parts;
}

char *join(char **parts, int count, const char *sep) {
    int total = 0;
    int seplen = strlen(sep);
    for (int i = 0; i < count; i++) total += strlen(parts[i]);
    total += seplen * (count - 1);
    char *out = malloc(total + 1);
    out[0] = '\0';
    for (int i = 0; i < count; i++) {
        if (i > 0) strcat(out, sep);
        strcat(out, parts[i]);
    }
    return out;
}

int str_contains(const char *hay, const char *needle) {
    return strstr(hay, needle) != NULL;
}

char *str_replace(const char *s, const char *old, const char *new_s) {
    int olen = strlen(old), nlen = strlen(new_s), slen = strlen(s);
    int cap = slen + 64;
    char *out = malloc(cap);
    int oi = 0;
    for (int i = 0; i <= slen - olen; ) {
        if (memcmp(s + i, old, olen) == 0) {
            while (oi + nlen >= cap) { cap *= 2; out = realloc(out, cap); }
            memcpy(out + oi, new_s, nlen);
            oi += nlen;
            i += olen;
        } else {
            if (oi + 1 >= cap) { cap *= 2; out = realloc(out, cap); }
            out[oi++] = s[i++];
        }
    }
    out[oi] = '\0';
    return out;
}

int main() {
    const char *base = "the quick brown fox jumps over the lazy dog";
    int count = 0;
    for (int i = 0; i < 1000; i++) {
        int n;
        char **words = split(base, ' ', &n);
        char *joined = join(words, n, "-");
        int has_fox = str_contains(joined, "fox");
        char *replaced = str_replace(joined, "fox", "cat");
        if (has_fox) count++;
        for (int j = 0; j < n; j++) free(words[j]);
        free(words);
        free(joined);
        free(replaced);
    }
    printf("%d\n", count);
    return 0;
}
