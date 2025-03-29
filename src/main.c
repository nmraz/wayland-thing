#include <math.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>

#include "protocols/xdg-shell.h"

struct wayland_thing_context {
    struct wl_display* display;
    struct wl_registry* registry;
    struct wl_compositor* compositor;
    struct xdg_wm_base* xdg_wm_base;
    struct wl_shm* shm;
};

static void registry_global_handler(void* data, struct wl_registry* registry,
                                    uint32_t name, const char* interface,
                                    uint32_t version) {
    struct wayland_thing_context* ctx = data;

    printf("new '%s' instance (version %u) bound at %u\n", interface, version,
           name);

    if (!strcmp(interface, "wl_compositor")) {
        ctx->compositor =
            wl_registry_bind(registry, name, &wl_compositor_interface, 1);
    } else if (!strcmp(interface, "xdg_wm_base")) {
        ctx->xdg_wm_base =
            wl_registry_bind(registry, name, &xdg_wm_base_interface, 1);
    } else if (!strcmp(interface, "wl_shm")) {
        ctx->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    }
}

static void registry_global_remove_handler(void* data,
                                           struct wl_registry* registry,
                                           uint32_t name) {
    printf("instance %u removed\n", name);
}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global_handler,
    .global_remove = registry_global_remove_handler,
};

static void xdg_ping_handler(void* data, struct xdg_wm_base* xdg_wm_base,
                             uint32_t serial) {
    xdg_wm_base_pong(xdg_wm_base, serial);
}

static const struct xdg_wm_base_listener xdg_listener = {
    .ping = xdg_ping_handler,
};

#define WINDOW_WIDTH 500
#define WINDOW_HEIGHT 500

#define ALIGN_UP(a, b) (((a) + (b) - 1) & -(b))

#define WINDOW_BUFFER_SIZE (WINDOW_WIDTH * WINDOW_HEIGHT * 4)
#define POOL_SIZE ALIGN_UP(2 * WINDOW_BUFFER_SIZE, 0x1000)

#define THROB_PERIOD_MS 2000
#define THROB_COLOR 0x0000ff

static void draw_window(uint32_t* framebuffer, size_t width, size_t height,
                        size_t stride, uint32_t frame_time_ms) {
    double t =
        (1. + sin(2. * M_PI * frame_time_ms / (double) THROB_PERIOD_MS)) * 0.5;

    // Cheap (approximate) linear -> sRGB conversion.
    double intensity = pow(t, 0.4545);
    uint32_t color = (uint32_t) (intensity * THROB_COLOR);

    for (size_t i = 0; i < height; i++) {
        for (size_t j = 0; j < width; j++) {
            framebuffer[stride * i + j] = color;
        }
    }
}

struct window_context {
    uint32_t seq;
    struct wl_surface* surface;
    struct wl_shm_pool* pool;
    uint8_t* pool_mapping;
};

static void present_frame(struct window_context* ctx, uint32_t frame_time);

static void frame_callback_handler(void* data, struct wl_callback* callback,
                                   uint32_t callback_data) {
    present_frame(data, callback_data);
};

static const struct wl_callback_listener frame_callback_listener = {
    .done = frame_callback_handler,
};

static void buffer_release_handler(void* data, struct wl_buffer* buffer) {
    // This is easiest for now.
    wl_buffer_destroy(buffer);
}

static const struct wl_buffer_listener buffer_listener = {
    .release = buffer_release_handler,
};

static void present_frame(struct window_context* ctx, uint32_t frame_time_ms) {
    // TODO: We assume buffers will always be released on time for
    // double-buffering here to be sufficient.
    uint32_t buffer_offset = ((ctx->seq++) & 1) * WINDOW_BUFFER_SIZE;

    // It's easiest to repeatedly create new buffers for now.
    struct wl_buffer* buffer = wl_shm_pool_create_buffer(
        ctx->pool, buffer_offset, WINDOW_WIDTH, WINDOW_HEIGHT, WINDOW_WIDTH,
        WL_SHM_FORMAT_XRGB8888);
    if (!buffer) {
        puts("failed to create buffer");
        exit(1);
    }

    wl_buffer_add_listener(buffer, &buffer_listener, NULL);

    struct wl_callback* frame_callback = wl_surface_frame(ctx->surface);
    if (!frame_callback) {
        puts("failed to request new frame callback");
        exit(1);
    }

    wl_callback_add_listener(frame_callback, &frame_callback_listener, ctx);

    draw_window((uint32_t*) (ctx->pool_mapping + buffer_offset), WINDOW_WIDTH,
                WINDOW_HEIGHT, WINDOW_HEIGHT, frame_time_ms);

    wl_surface_attach(ctx->surface, buffer, 0, 0);
    wl_surface_damage(ctx->surface, 0, 0, WINDOW_WIDTH, WINDOW_HEIGHT);
    wl_surface_commit(ctx->surface);
}

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

    if (!ctx.shm) {
        puts("failed to get shm object");
        return 1;
    }

    if (!ctx.xdg_wm_base) {
        puts("failed to get XDG shell object");
        return 1;
    }

    xdg_wm_base_add_listener(ctx.xdg_wm_base, &xdg_listener, NULL);

    int pool_fd = memfd_create("wayland_thing_pool", MFD_CLOEXEC);
    if (pool_fd == -1) {
        puts("failed to create pool fd");
        return 1;
    }

    if (ftruncate(pool_fd, POOL_SIZE) != 0) {
        puts("failed to allocate pool backing memory");
        return 1;
    }

    struct wl_shm_pool* pool = wl_shm_create_pool(ctx.shm, pool_fd, POOL_SIZE);
    if (!pool) {
        puts("failed to create pool");
        return 1;
    }

    struct wl_surface* surface = wl_compositor_create_surface(ctx.compositor);
    if (!surface) {
        puts("failed to create surface");
        return 1;
    }

    struct xdg_surface* xdg_surface =
        xdg_wm_base_get_xdg_surface(ctx.xdg_wm_base, surface);
    if (!xdg_surface) {
        puts("failed to get XDG surface");
        return 1;
    }

    struct xdg_toplevel* toplevel = xdg_surface_get_toplevel(xdg_surface);
    if (!toplevel) {
        puts("failed to set surface as toplevel");
        return 1;
    }

    xdg_toplevel_set_title(toplevel, "Wayland Thing");

    uint8_t* pool_mapping =
        mmap(NULL, POOL_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, pool_fd, 0);
    if (pool_mapping == MAP_FAILED) {
        puts("failed to map pool");
        return 1;
    }

    struct window_context window_ctx = {
        .surface = surface,
        .pool = pool,
        .pool_mapping = pool_mapping,
    };

    // Draw the first frame manually so the window is actually visible. Once it
    // is, we'll start getting frame callbacks from the compositor.
    present_frame(&window_ctx, 200);
    for (;;) {
        wl_display_dispatch(display);
        int err = wl_display_get_error(display);
        if (err != 0) {
            printf("wayland error: %s\n", strerror(err));
            return 1;
        }
    }

    munmap(pool_mapping, POOL_SIZE);

    xdg_toplevel_destroy(toplevel);
    xdg_surface_destroy(xdg_surface);
    wl_surface_destroy(surface);
    wl_shm_pool_destroy(pool);

    close(pool_fd);

    xdg_wm_base_destroy(ctx.xdg_wm_base);
    wl_shm_destroy(ctx.shm);
    wl_compositor_destroy(ctx.compositor);
    wl_registry_destroy(ctx.registry);
    wl_display_disconnect(ctx.display);

    return 0;
}
