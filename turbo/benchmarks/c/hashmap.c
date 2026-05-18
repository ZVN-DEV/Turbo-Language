#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define BUCKETS 131071

typedef struct Entry {
    char *key;
    char *val;
    struct Entry *next;
} Entry;

Entry *table[BUCKETS];

unsigned long hash(const char *s) {
    unsigned long h = 5381;
    while (*s) h = h * 33 + (unsigned char)*s++;
    return h;
}

void hm_set(const char *key, const char *val) {
    unsigned long idx = hash(key) % BUCKETS;
    Entry *e = table[idx];
    while (e) {
        if (strcmp(e->key, key) == 0) {
            free(e->val);
            e->val = strdup(val);
            return;
        }
        e = e->next;
    }
    e = malloc(sizeof(Entry));
    e->key = strdup(key);
    e->val = strdup(val);
    e->next = table[idx];
    table[idx] = e;
}

const char *hm_get(const char *key) {
    unsigned long idx = hash(key) % BUCKETS;
    Entry *e = table[idx];
    while (e) {
        if (strcmp(e->key, key) == 0) return e->val;
        e = e->next;
    }
    return NULL;
}

int main() {
    char kbuf[20], vbuf[20];
    for (int i = 0; i < 100000; i++) {
        sprintf(kbuf, "%d", i);
        sprintf(vbuf, "%d", i * 7);
        hm_set(kbuf, vbuf);
    }
    int found = 0;
    for (int i = 0; i < 100000; i++) {
        sprintf(kbuf, "%d", i);
        sprintf(vbuf, "%d", i * 7);
        const char *v = hm_get(kbuf);
        if (v && strcmp(v, vbuf) == 0) found++;
    }
    printf("%d\n", found);
    return 0;
}
