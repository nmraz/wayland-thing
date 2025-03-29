#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <wayland-client.h>

struct wayland_thing_context {
    struct wl_display* display;
    struct wl_registry* registry;
    struct wl_compositor* compositor;
};

static void global_handler(void* data, struct wl_registry* registry,
                           uint32_t name, const char* interface,
                           uint32_t version) {
    struct wayland_thing_context* ctx = data;

    printf("new '%s' instance (version %u) bound at %u\n", interface, version,
           name);

    if (!strcmp(interface, "wl_compositor")) {
        ctx->compositor =
            wl_registry_bind(registry, name, &wl_compositor_interface, 1);
    }
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

    struct wayland_thing_context ctx = {
        .display = display,
        .registry = registry,
    };

    wl_registry_add_listener(registry, &registry_listener, &ctx);

    // Wait for notifications about all current globals to be handled.
    wl_display_roundtrip(display);

    if (!ctx.compositor) {
        puts("failed to get compositor object");
        return 1;
    }

    struct wl_surface* surface = wl_compositor_create_surface(ctx.compositor);
    if (!surface) {
        puts("failed to create surface");
        return 1;
    }

    puts("created surface");

    wl_registry_destroy(registry);
    wl_display_disconnect(display);

    return 0;
}
