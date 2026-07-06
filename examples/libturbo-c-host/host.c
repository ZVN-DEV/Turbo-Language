#include "libturbo.h"

#include <inttypes.h>
#include <stdio.h>
#include <string.h>

static int64_t host_add(int64_t value, const char *label) {
    if (strcmp(label, "from turbo") != 0) {
        fprintf(stderr, "unexpected label from Turbo: %s\n", label);
        return -1;
    }
    return value + 5;
}

static const char *host_greet(const char *name) {
    if (strcmp(name, "Turbo") != 0) {
        return "unexpected guest";
    }
    return "hello Turbo from C host";
}

static int fail(TurboVm *vm, const char *step) {
    const char *error = turbo_vm_last_error(vm);
    fprintf(stderr, "%s failed: %s\n", step, error ? error : "unknown error");
    turbo_vm_free(vm);
    return 1;
}

int main(void) {
    TurboVm *vm = turbo_vm_new();
    if (!vm) {
        fprintf(stderr, "turbo_vm_new failed\n");
        return 1;
    }

    if (!turbo_vm_register_host_fn(vm, "host_add", (const void *)host_add)) {
        return fail(vm, "register host_add");
    }
    if (!turbo_vm_register_host_fn(vm, "host_greet", (const void *)host_greet)) {
        return fail(vm, "register host_greet");
    }

    const char *source =
        "@unsafe\n"
        "extern \"C\" {\n"
        "    fn host_add(value: i64, label: str) -> i64\n"
        "    fn host_greet(name: str) -> str\n"
        "}\n"
        "\n"
        "fn answer() -> i64 {\n"
        "    host_add(37, \"from turbo\")\n"
        "}\n"
        "\n"
        "fn message() -> str {\n"
        "    host_greet(\"Turbo\")\n"
        "}\n"
        "\n"
        "fn main() {\n"
        "    assert_eq(answer(), 42)\n"
        "    assert_eq(len(message()), 23)\n"
        "}\n";

    if (!turbo_eval(vm, source)) {
        return fail(vm, "turbo_eval");
    }

    int64_t answer = 0;
    if (!turbo_call_i64(vm, "answer", &answer)) {
        return fail(vm, "turbo_call_i64");
    }

    const char *message = turbo_call_str(vm, "message");
    if (!message) {
        return fail(vm, "turbo_call_str");
    }

    printf("answer=%" PRId64 "\n", answer);
    printf("message=%s\n", message);

    turbo_vm_free(vm);
    return 0;
}
