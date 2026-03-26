/*
 * Turbo Runtime Library
 *
 * Provides runtime functions for AOT-compiled Turbo binaries.
 * These are the same functions that the JIT links as function pointers,
 * but compiled as real symbols for the system linker to resolve.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void rt_print_str(const char *s) {
    if (s)
        printf("%s\n", s);
    else
        printf("\n");
}

void rt_print_i64(long long n) {
    printf("%lld\n", n);
}

void rt_print_f64(double n) {
    printf("%g\n", n);
}

void rt_print_bool(char b) {
    printf("%s\n", b ? "true" : "false");
}

void rt_panic(const char *msg) {
    if (msg)
        fprintf(stderr, "panic: %s\n", msg);
    else
        fprintf(stderr, "panic: explicit panic\n");
    exit(1);
}

void rt_assert_fail(const char *msg) {
    if (msg)
        fprintf(stderr, "assertion failed: %s\n", msg);
    else
        fprintf(stderr, "assertion failed\n");
    exit(1);
}

void rt_div_by_zero(void) {
    fprintf(stderr, "runtime error: division by zero\n");
    exit(1);
}

void rt_int_overflow(void) {
    fprintf(stderr, "runtime error: integer overflow\n");
    exit(1);
}

const char* rt_str_concat(const char *a, const char *b) {
    size_t a_len = a ? strlen(a) : 0;
    size_t b_len = b ? strlen(b) : 0;
    char *result = malloc(a_len + b_len + 1);
    if (a) memcpy(result, a, a_len);
    if (b) memcpy(result + a_len, b, b_len);
    result[a_len + b_len] = '\0';
    return result;
}

char rt_str_eq(const char *a, const char *b) {
    if (!a && !b) return 1;
    if (!a || !b) return 0;
    return strcmp(a, b) == 0 ? 1 : 0;
}

void* rt_array_alloc(long long len) {
    size_t total = 8 + len * 8;
    void *ptr = calloc(1, total);
    *(long long*)ptr = len;
    return ptr;
}

long long rt_array_get(const void *arr, long long index) {
    long long len = *(const long long*)arr;
    if (index < 0 || index >= len) {
        fprintf(stderr, "runtime error: array index %lld out of bounds (length %lld)\n", index, len);
        exit(1);
    }
    return ((const long long*)arr)[1 + index];
}

long long rt_array_len(const void *arr) {
    return *(const long long*)arr;
}

long long rt_str_len(const char *s) {
    return s ? (long long)strlen(s) : 0;
}

void* rt_struct_alloc(long long num_fields) {
    size_t size = num_fields * 8;
    if (size < 8) size = 8;
    return calloc(1, size);
}

const char* rt_i64_to_str(long long n) {
    char *buf = malloc(32);
    snprintf(buf, 32, "%lld", n);
    return buf;
}

const char* rt_f64_to_str(double n) {
    char *buf = malloc(32);
    snprintf(buf, 32, "%g", n);
    return buf;
}

const char* rt_bool_to_str(char b) {
    if (b) {
        char *buf = malloc(5);
        memcpy(buf, "true", 5);
        return buf;
    } else {
        char *buf = malloc(6);
        memcpy(buf, "false", 6);
        return buf;
    }
}

/* Entry point: calls Turbo's main and returns 0 */
extern void turbo_main(void);
int main(void) {
    turbo_main();
    return 0;
}
