/*
 * IBDE toolkit spike — Phase 0 go/no-go evidence (ADR-0004).
 *
 * A wlr-layer-shell top-bar client: one wl_shm buffer, cairo + pango,
 * animated rounded rect + text, frame-callback driven. Reports FPS and
 * RSS every second. Proves the software-rendering path end-to-end and
 * measures its cost.
 *
 * Modes:
 *   spike                 — connect to compositor, run 10 s
 *   spike --seconds N     — run N seconds
 *   spike --offscreen N   — render N frames with no compositor (pure cost)
 *
 * Apache-2.0, IBDE project.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdbool.h>
#include <math.h>
#include <time.h>
#include <poll.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/syscall.h>

#include <wayland-client.h>
#include <cairo/cairo.h>
#include <pango/pangocairo.h>

#include "layer-shell-protocol.h"

#define BAR_H 32

struct buf {
    int w, h, stride, size;
    void *data;
    int fd;
    struct wl_shm_pool *pool;
    struct wl_buffer *wlbuf;
    cairo_surface_t *cs;
    cairo_t *cr;
};

struct state {
    struct wl_display *disp;
    struct wl_registry *reg;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct zwlr_layer_shell_v1 *shell;
    struct wl_surface *surface;
    struct zwlr_layer_surface_v1 *ls;
    struct buf buf;
    int cfg_w, cfg_h;
    bool configured, running, have_shell;
    int64_t t0_ms;
    int frames;
    double seconds;
    PangoFontDescription *font;
};

static int64_t now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static long rss_kb(void)
{
    FILE *f = fopen("/proc/self/statm", "r");
    if (!f) return -1;
    long size, resident;
    if (fscanf(f, "%ld %ld", &size, &resident) != 2) resident = 0;
    fclose(f);
    return resident * (4096 / 1024);
}

static void die(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fprintf(stderr, "\n");
    exit(1);
}

/* ---------- buffers ---------- */

static void buffer_destroy(struct buf *b)
{
    if (b->cr) cairo_destroy(b->cr);
    if (b->cs) cairo_surface_destroy(b->cs);
    if (b->wlbuf) wl_buffer_destroy(b->wlbuf);
    if (b->pool) wl_shm_pool_destroy(b->pool);
    if (b->data && b->data != MAP_FAILED) munmap(b->data, b->size);
    if (b->fd >= 0) close(b->fd);
    memset(b, 0, sizeof(*b));
    b->fd = -1;
}

static void buffer_create(struct state *st, int w, int h)
{
    struct buf *b = &st->buf;
    buffer_destroy(b);
    b->w = w; b->h = h;
    b->stride = w * 4;
    b->size = b->stride * h;
    b->fd = syscall(SYS_memfd_create, "ibde-spike", 1 /* MFD_CLOEXEC */);
    if (b->fd < 0) die("memfd_create failed");
    if (ftruncate(b->fd, b->size) < 0) die("ftruncate failed");
    b->data = mmap(NULL, b->size, PROT_READ | PROT_WRITE, MAP_SHARED, b->fd, 0);
    if (b->data == MAP_FAILED) die("mmap failed");
    b->pool = wl_shm_create_pool(st->shm, b->fd, b->size);
    b->wlbuf = wl_shm_pool_create_buffer(b->pool, 0, w, h, b->stride,
                                         WL_SHM_FORMAT_ARGB8888);
    b->cs = cairo_image_surface_create_for_data(b->data, CAIRO_FORMAT_ARGB32,
                                                w, h, b->stride);
    b->cr = cairo_create(b->cs);
}

/* ---------- rendering ---------- */

static void render(struct state *st, cairo_t *cr, int w, int h, double t)
{
    double pulse = 0.80 + 0.20 * sin(t * 2.0);

    cairo_save(cr);
    cairo_set_operator(cr, CAIRO_OPERATOR_CLEAR);
    cairo_paint(cr);
    cairo_restore(cr);

    /* bar background: translucent graphite */
    cairo_set_source_rgba(cr, 0.92, 0.92, 0.94, 0.88);
    cairo_set_antialias(cr, CAIRO_ANTIALIAS_BEST);
    cairo_new_path(cr);
    cairo_move_to(cr, 0, 0);
    cairo_line_to(cr, w, 0);
    cairo_line_to(cr, w, h);
    cairo_line_to(cr, 0, h);
    cairo_close_path(cr);
    cairo_fill(cr);

    /* animated accent pill, left side */
    double pill_w = 180 + 40 * sin(t * 1.3);
    double r = h / 2.0;
    cairo_set_source_rgba(cr, 0.35, 0.38, 0.45, pulse);
    cairo_new_path(cr);
    cairo_arc(cr, 16 + r, h / 2.0, r, M_PI / 2, 3 * M_PI / 2);
    cairo_arc(cr, 16 + pill_w - r, h / 2.0, r, -M_PI / 2, M_PI / 2);
    cairo_close_path(cr);
    cairo_fill(cr);

    /* text */
    char text[128];
    snprintf(text, sizeof text, "IBDE spike  %dx%d  frame %d", w, h, st->frames);
    PangoLayout *pl = pango_cairo_create_layout(cr);
    pango_layout_set_font_description(pl, st->font);
    pango_layout_set_text(pl, text, -1);
    int tw, th;
    pango_layout_get_pixel_size(pl, &tw, &th);
    cairo_set_source_rgba(cr, 0.1, 0.1, 0.12, 1.0);
    cairo_move_to(cr, 220, (h - th) / 2.0);
    pango_cairo_show_layout(cr, pl);
    g_object_unref(pl);
}

/* ---------- wayland plumbing ---------- */

static void frame_done(void *data, struct wl_callback *cb, uint32_t t);

static const struct wl_callback_listener frame_listener = {
    .done = frame_done,
};

static void frame_done(void *data, struct wl_callback *cb, uint32_t t)
{
    (void)t;
    struct state *st = data;
    wl_callback_destroy(cb);

    if (!st->running) return;

    double tsec = (now_ms() - st->t0_ms) / 1000.0;
    if (tsec >= st->seconds) {
        st->running = false;
        return;
    }

    static int64_t last_report = 0;
    if (last_report == 0) last_report = st->t0_ms;
    st->frames++;

    render(st, st->buf.cr, st->buf.w, st->buf.h, tsec);
    wl_surface_attach(st->surface, st->buf.wlbuf, 0, 0);
    wl_surface_damage_buffer(st->surface, 0, 0, st->buf.w, st->buf.h);
    wl_surface_commit(st->surface);

    int64_t now = now_ms();
    if (now - last_report >= 1000) {
        printf("STATS fps=%d rss_kb=%ld t=%.1f\n",
               st->frames, rss_kb(), tsec);
        fflush(stdout);
        st->frames = 0;
        last_report = now;
    }

    cb = wl_surface_frame(st->surface);
    wl_callback_add_listener(cb, &frame_listener, st);
    wl_surface_commit(st->surface);
    wl_display_flush(st->disp);
}

static void ls_configure(void *data, struct zwlr_layer_surface_v1 *ls,
                         uint32_t serial, uint32_t w, uint32_t h)
{
    struct state *st = data;
    zwlr_layer_surface_v1_ack_configure(ls, serial);
    st->cfg_w = w; st->cfg_h = h;
    if (w > 0 && h > 0)
        buffer_create(st, w, h);
    st->configured = true;
}

static void ls_closed(void *data, struct zwlr_layer_surface_v1 *ls)
{
    (void)ls;
    ((struct state *)data)->running = false;
}

static const struct zwlr_layer_surface_v1_listener ls_listener = {
    .configure = ls_configure,
    .closed = ls_closed,
};

static void reg_global(void *data, struct wl_registry *reg, uint32_t name,
                       const char *iface, uint32_t ver)
{
    (void)ver;
    struct state *st = data;
    if (!strcmp(iface, wl_compositor_interface.name))
        st->compositor = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
    else if (!strcmp(iface, wl_shm_interface.name))
        st->shm = wl_registry_bind(reg, name, &wl_shm_interface, 1);
    else if (!strcmp(iface, zwlr_layer_shell_v1_interface.name)) {
        st->shell = wl_registry_bind(reg, name, &zwlr_layer_shell_v1_interface, 1);
        st->have_shell = true;
    }
}

static void reg_remove(void *data, struct wl_registry *reg, uint32_t name)
{
    (void)data; (void)reg; (void)name;
}

static const struct wl_registry_listener reg_listener = {
    .global = reg_global,
    .global_remove = reg_remove,
};

/* ---------- offscreen mode ---------- */

static int run_offscreen(int frames)
{
    int w = 1280, h = BAR_H;
    unsigned char *pix = calloc(1, (size_t)w * h * 4);
    cairo_surface_t *cs = cairo_image_surface_create_for_data(
        pix, CAIRO_FORMAT_ARGB32, w, h, w * 4);
    cairo_t *cr = cairo_create(cs);
    struct state st = { .frames = 0 };
    st.font = pango_font_description_from_string("Sans 12");

    int64_t t0 = now_ms();
    for (int i = 0; i < frames; i++)
        render(&st, cr, w, h, i / 60.0);
    int64_t dt = now_ms() - t0;

    printf("OFFSCREEN frames=%d total_ms=%ld avg_ms=%.3f rss_kb=%ld\n",
           frames, (long)dt, (double)dt / frames, rss_kb());

    cairo_destroy(cr);
    cairo_surface_destroy(cs);
    free(pix);
    pango_font_description_free(st.font);
    return 0;
}

/* ---------- main ---------- */

int main(int argc, char **argv)
{
    double seconds = 10;
    int offscreen = 0;

    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--seconds") && i + 1 < argc)
            seconds = atof(argv[++i]);
        else if (!strcmp(argv[i], "--offscreen") && i + 1 < argc)
            offscreen = atoi(argv[++i]);
        else
            die("usage: spike [--seconds N] | [--offscreen N]");
    }

    if (offscreen > 0)
        return run_offscreen(offscreen);

    struct state st;
    memset(&st, 0, sizeof(st));
    st.seconds = seconds;
    st.running = true;
    st.buf.fd = -1;

    st.disp = wl_display_connect(NULL);
    if (!st.disp) die("cannot connect to a Wayland compositor "
                      "(WAYLAND_DISPLAY set?)");

    st.reg = wl_display_get_registry(st.disp);
    wl_registry_add_listener(st.reg, &reg_listener, &st);
    wl_display_roundtrip(st.disp);

    if (!st.compositor) die("no wl_compositor");
    if (!st.shm) die("no wl_shm");
    if (!st.have_shell) die("compositor lacks wlr-layer-shell "
                            "(need wlroots-based: labwc/sway)");

    st.font = pango_font_description_from_string("Sans 12");

    st.surface = wl_compositor_create_surface(st.compositor);
    st.ls = zwlr_layer_shell_v1_get_layer_surface(
        st.shell, st.surface, NULL, ZWLR_LAYER_SHELL_V1_LAYER_TOP,
        "ibde-spike");
    zwlr_layer_surface_v1_add_listener(st.ls, &ls_listener, &st);
    zwlr_layer_surface_v1_set_anchor(
        st.ls, ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP |
                   ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT |
                   ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT);
    zwlr_layer_surface_v1_set_size(st.ls, 0, BAR_H);
    zwlr_layer_surface_v1_set_exclusive_zone(st.ls, BAR_H);
    wl_surface_commit(st.surface);
    wl_display_roundtrip(st.disp);

    if (!st.configured) die("no configure arrived");
    if (!st.buf.wlbuf)
        buffer_create(&st, 1280, BAR_H);

    /* Initial frame: commit with damage so the compositor repaints and
       starts delivering frame callbacks (headless needs the first push). */
    st.t0_ms = now_ms();
    render(&st, st.buf.cr, st.buf.w, st.buf.h, 0.0);
    wl_surface_attach(st.surface, st.buf.wlbuf, 0, 0);
    wl_surface_damage_buffer(st.surface, 0, 0, st.buf.w, st.buf.h);
    wl_surface_commit(st.surface);
    wl_display_flush(st.disp);

    struct wl_callback *cb = wl_surface_frame(st.surface);
    wl_callback_add_listener(cb, &frame_listener, &st);
    wl_surface_commit(st.surface);
    wl_display_flush(st.disp);

    /* poll-based loop: guarantees exit even if callbacks stop */
    while (st.running) {
        struct pollfd pfd = { wl_display_get_fd(st.disp), POLLIN };
        int ready = poll(&pfd, 1, 200);
        if (ready < 0) die("poll failed");
        if (ready > 0 && wl_display_dispatch(st.disp) == -1)
            die("display dispatch error");
        if ((now_ms() - st.t0_ms) / 1000.0 >= st.seconds) break;
    }

    printf("SUMMARY avg_over=%.1fs rss_kb=%ld\n", st.seconds, rss_kb());
    buffer_destroy(&st.buf);
    pango_font_description_free(st.font);
    wl_display_disconnect(st.disp);
    return 0;
}
