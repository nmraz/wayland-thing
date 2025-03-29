#include <stddef.h>
#include <stdio.h>
#include <wayland-client.h>

static void global_handler(void* data, struct wl_registry* registry,
                           uint32_t name, const char* interface,
                           uint32_t version) {
    printf("new '%s' instance (version %u) bound at %u\n", interface, version,
           name);
}

static void global_remove_handler(void* data, struct wl_registry* registry,
                                  uint32_t name) {
    printf("instance %u removed\n", name);
}

static const struct wl_registry_listener registry_listener = {
    .global = global_handler,
    .global_remove = global_remove_handler,
};

int main(void) {
    struct wl_display* display = wl_display_connect(NULL);
    if (!display) {
        puts("failed to connect");
        return 1;
    }

    struct wl_registry* registry = wl_display_get_registry(display);
    if (!registry) {
        puts("failed to get registry");
        return 1;
    }

    wl_registry_add_listener(registry, &registry_listener, NULL);

    // Wait for notifications about all current globals to be handled.
    wl_display_roundtrip(display);

    wl_registry_destroy(registry);
    wl_display_disconnect(display);

    return 0;
}
