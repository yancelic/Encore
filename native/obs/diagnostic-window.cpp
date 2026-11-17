// SPDX-License-Identifier: GPL-3.0-only
// A separate, controlled desktop window for the real XComposite capture test.
#include <gtk/gtk.h>
#include <cmath>

int main(int argc, char **argv) {
    gtk_init(&argc, &argv);
    g_set_application_name("Encore Capture Fixture");
    auto *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "Encore Capture Fixture");
    gtk_window_set_default_size(GTK_WINDOW(window), 640, 360);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), nullptr);
    g_signal_connect(window, "draw", G_CALLBACK(+[](GtkWidget *widget, cairo_t *canvas, gpointer) -> gboolean {
        const auto time = static_cast<double>(g_get_monotonic_time()) / G_USEC_PER_SEC;
        cairo_set_source_rgb(canvas, 0.5 + 0.4 * std::sin(time * 5), 0.15, 0.65);
        cairo_paint(canvas);
        cairo_set_source_rgb(canvas, 1.0, 1.0, 1.0);
        cairo_rectangle(canvas, 20 + std::fmod(time * 100, 560), 140, 40, 40);
        cairo_fill(canvas);
        (void)widget;
        return TRUE;
    }), nullptr);
    g_timeout_add(33, +[](gpointer window) -> gboolean { gtk_widget_queue_draw(GTK_WIDGET(window)); return TRUE; }, window);
    g_timeout_add_seconds(60, +[](gpointer) -> gboolean { gtk_main_quit(); return FALSE; }, nullptr);
    gtk_widget_show_all(window);
    gtk_main();
}
