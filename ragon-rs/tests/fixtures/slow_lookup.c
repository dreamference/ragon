// Process-local resolver for the launcher's slow DNS test (glibc).
#define _GNU_SOURCE
#include <dlfcn.h>
#include <netdb.h>
#include <string.h>
#include <unistd.h>

int getaddrinfo(const char *name, const char *service, const struct addrinfo *hints,
                struct addrinfo **result) {
    if (name && strcmp(name, "fixture.local") == 0) {
        sleep(10);
        name = "127.0.0.1";
    }
    // Never pass a hostname to the system resolver.
    struct addrinfo numeric = hints ? *hints : (struct addrinfo){0};
    numeric.ai_flags |= AI_NUMERICHOST;
    int (*resolve)(const char *, const char *, const struct addrinfo *, struct addrinfo **) =
        dlsym(RTLD_NEXT, "getaddrinfo");
    return resolve(name, service, &numeric, result);
}
