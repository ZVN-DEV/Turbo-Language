/* Word-frequency count baseline (C).
 *
 * Reads the file given as argv[1] (or $WORDCOUNT_INPUT), tokenizes on ASCII
 * whitespace, counts word frequencies in an open-addressing hash table, then
 * prints the top-20 words by (count desc, word asc) followed by a final
 * "TOTAL <words> <unique>" line. Output must match wordcount.tb byte-for-byte. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define TOP_N 20

typedef struct {
    char *key;
    long long count;
} Entry;

static Entry *table;
static long long cap;
static long long used;

static unsigned long hash_str(const char *s) {
    /* djb2 */
    unsigned long h = 5381;
    int c;
    while ((c = (unsigned char)*s++)) {
        h = ((h << 5) + h) + (unsigned long)c;
    }
    return h;
}

static void table_init(long long initial) {
    cap = initial;
    used = 0;
    table = (Entry *)calloc((size_t)cap, sizeof(Entry));
    if (!table) {
        fprintf(stderr, "out of memory\n");
        exit(1);
    }
}

static void table_resize(void);

static void table_inc(const char *key) {
    if ((used + 1) * 4 >= cap * 3) {
        table_resize();
    }
    unsigned long h = hash_str(key) % (unsigned long)cap;
    while (table[h].key) {
        if (strcmp(table[h].key, key) == 0) {
            table[h].count++;
            return;
        }
        h = (h + 1) % (unsigned long)cap;
    }
    table[h].key = strdup(key);
    table[h].count = 1;
    used++;
}

static void table_resize(void) {
    Entry *old = table;
    long long old_cap = cap;
    cap *= 2;
    table = (Entry *)calloc((size_t)cap, sizeof(Entry));
    if (!table) {
        fprintf(stderr, "out of memory\n");
        exit(1);
    }
    used = 0;
    for (long long i = 0; i < old_cap; i++) {
        if (old[i].key) {
            unsigned long h = hash_str(old[i].key) % (unsigned long)cap;
            while (table[h].key) {
                h = (h + 1) % (unsigned long)cap;
            }
            table[h].key = old[i].key;
            table[h].count = old[i].count;
            used++;
        }
    }
    free(old);
}

static int cmp_entry(const void *a, const void *b) {
    const Entry *ea = (const Entry *)a;
    const Entry *eb = (const Entry *)b;
    if (ea->count != eb->count) {
        return ea->count < eb->count ? 1 : -1; /* count descending */
    }
    return strcmp(ea->key, eb->key); /* word ascending */
}

static int is_space(int c) {
    return c == ' ' || c == '\n' || c == '\t' || c == '\r' || c == '\f' || c == '\v';
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : getenv("WORDCOUNT_INPUT");
    if (!path) {
        path = "wordcount_input.txt";
    }

    FILE *f = fopen(path, "rb");
    if (!f) {
        fprintf(stderr, "cannot open %s\n", path);
        return 1;
    }
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *buf = (char *)malloc((size_t)sz + 1);
    if (!buf) {
        fprintf(stderr, "out of memory\n");
        return 1;
    }
    size_t got = fread(buf, 1, (size_t)sz, f);
    buf[got] = '\0';
    fclose(f);

    table_init(1 << 12);

    long long total = 0;
    size_t i = 0;
    while (i < got) {
        while (i < got && is_space((unsigned char)buf[i])) {
            i++;
        }
        size_t start = i;
        while (i < got && !is_space((unsigned char)buf[i])) {
            i++;
        }
        if (i > start) {
            char saved = buf[i];
            buf[i] = '\0';
            table_inc(buf + start);
            buf[i] = saved;
            total++;
        }
    }

    Entry *list = (Entry *)malloc((size_t)used * sizeof(Entry));
    long long m = 0;
    for (long long j = 0; j < cap; j++) {
        if (table[j].key) {
            list[m++] = table[j];
        }
    }
    qsort(list, (size_t)m, sizeof(Entry), cmp_entry);

    long long limit = m < TOP_N ? m : TOP_N;
    for (long long j = 0; j < limit; j++) {
        printf("%s %lld\n", list[j].key, list[j].count);
    }
    printf("TOTAL %lld %lld\n", total, m);
    return 0;
}
