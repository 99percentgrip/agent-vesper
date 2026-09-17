// vb-xtest.c — minimal X11 keystroke sender for the Bridge Console channel.
// Links against system libX11/libXtst (runtime .so present on Fedora 44).
// Purpose: host-side driver can open the Resolve Console and type the one
// bootstrap line without a human paste. This is a PROBE-grade tool, not
// production code: Bridge production will use a Rust x11 client crate.
//
// Usage: vb-xtest "text to type"
// Sends XTestFakeKeyEvent keystrokes to the focused window after 1.5s delay.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/keysym.h>
#include <X11/extensions/XTest.h>

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: %s \"text\"\n", argv[0]); return 2; }
    Display *d = XOpenDisplay(NULL);
    if (!d) { fprintf(stderr, "no X display\n"); return 1; }

    printf("typing in 1.5s — focus the target window now\n");
    fflush(stdout);
    usleep(1500000);

    for (const char *p = argv[1]; *p; p++) {
        char c = *p;
        KeyCode kc = XKeysymToKeycode(d, (KeySym)c);
        if (!kc) { fprintf(stderr, "no keycode for '%c'\n", c); continue; }
        int shift = (c >= 'A' && c <= 'Z') || strchr("!@#$%^&*()_+{}|:\"<>?", c);
        if (shift) XTestFakeKeyEvent(d, XKeysymToKeycode(d, XK_Shift_L), True, 0);
        XTestFakeKeyEvent(d, kc, True, 0);
        XTestFakeKeyEvent(d, kc, False, 0);
        if (shift) XTestFakeKeyEvent(d, XKeysymToKeycode(d, XK_Shift_L), False, 0);
        XFlush(d);
        usleep(15000); // 15ms/char
    }
    // Enter
    XTestFakeKeyEvent(d, XKeysymToKeycode(d, XK_Return), True, 0);
    XTestFakeKeyEvent(d, XKeysymToKeycode(d, XK_Return), False, 0);
    XFlush(d);
    printf("done\n");
    XCloseDisplay(d);
    return 0;
}
