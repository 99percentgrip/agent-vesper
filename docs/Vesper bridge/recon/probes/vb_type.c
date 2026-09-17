// vb_type.c — XTest typer with a COMPLETE US-layout table (shift-aware).
// Fixes vb_dl's dropped symbols: _ ( ) " ! etc.
// Usage: vb_type "text" [--enter]
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <dlfcn.h>

typedef struct _XDisplay Display;

// US layout: char -> {keysym, shift}
typedef struct { const char *ch; unsigned long keysym; int shift; } KeyMap;
static const KeyMap MAP[] = {
    {"a",0x61,0},{"b",0x62,0},{"c",0x63,0},{"d",0x64,0},{"e",0x65,0},
    {"f",0x66,0},{"g",0x67,0},{"h",0x68,0},{"i",0x69,0},{"j",0x6a,0},
    {"k",0x6b,0},{"l",0x6c,0},{"m",0x6d,0},{"n",0x6e,0},{"o",0x6f,0},
    {"p",0x70,0},{"q",0x71,0},{"r",0x72,0},{"s",0x73,0},{"t",0x74,0},
    {"u",0x75,0},{"v",0x76,0},{"w",0x77,0},{"x",0x78,0},{"y",0x79,0},
    {"z",0x7a,0},
    {"A",0x41,1},{"B",0x42,1},{"C",0x43,1},{"D",0x44,1},{"E",0x45,1},
    {"F",0x46,1},{"G",0x47,1},{"H",0x48,1},{"I",0x49,1},{"J",0x4a,1},
    {"K",0x4b,1},{"L",0x4c,1},{"M",0x4d,1},{"N",0x4e,1},{"O",0x4f,1},
    {"P",0x50,1},{"Q",0x51,1},{"R",0x52,1},{"S",0x53,1},{"T",0x54,1},
    {"U",0x55,1},{"V",0x56,1},{"W",0x57,1},{"X",0x58,1},{"Y",0x59,1},
    {"Z",0x5a,1},
    {"1",0x31,0},{"2",0x32,0},{"3",0x33,0},{"4",0x34,0},{"5",0x35,0},
    {"6",0x36,0},{"7",0x37,0},{"8",0x38,0},{"9",0x39,0},{"0",0x30,0},
    {" ",0x20,0},
    {"!",0x21,1},{"@",0x40,1},{"#",0x23,1},{"$",0x24,1},{"%",0x25,1},
    {"^",0x5e,1},{"&",0x26,1},{"*",0x2a,1},{"(",0x28,1},{")",0x29,1},
    {"-",0x2d,0},{"_",0x5f,1},{"=",0x3d,0},{"+",0x2b,1},
    {"[",0x5b,0},{"]",0x5d,0},{"{",0x7b,1},{"}",0x7d,1},
    {";",0x3b,0},{":",0x3a,1},{"'",0x27,0},{"\"",0x22,1},
    {",",0x2c,0},{"<",0x3c,1},{".",0x2e,0},{">",0x3e,1},
    {"/",0x2f,0},{"?",0x3f,1},{"\\",0x5c,0},{"|",0x7c,1},
    {"`",0x60,0},{"~",0x7e,1},
};
#define SHIFT_KS 0xffe1   // XK_Shift_L
#define RETURN_KS 0xff0d  // XK_Return

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: %s \"text\" [--no-enter]\n", argv[0]); return 2; }
    void *x11 = dlopen("libX11.so.6", RTLD_NOW);
    void *xtst = dlopen("libXtst.so.6", RTLD_NOW);
    if (!x11 || !xtst) { fprintf(stderr, "dlopen fail\n"); return 1; }
    Display *(*open_)(const char*) = dlsym(x11, "XOpenDisplay");
    unsigned char (*sym2code)(Display*, unsigned long) = dlsym(x11, "XKeysymToKeycode");
    int (*flush_)(Display*) = dlsym(x11, "XFlush");
    int (*fake_)(Display*, unsigned char, int, unsigned long) = dlsym(xtst, "XTestFakeKeyEvent");
    if (!open_ || !sym2code || !flush_ || !fake_) { fprintf(stderr, "dlsym fail\n"); return 1; }

    Display *d = open_(NULL);
    if (!d) { fprintf(stderr, "no display\n"); return 1; }
    unsigned char shift_kc = sym2code(d, SHIFT_KS);

    int send_enter = 1;
    if (argc > 2 && strcmp(argv[2], "--no-enter") == 0) send_enter = 0;

    printf("typing %zu chars in 1.0s (focus target)\n", strlen(argv[1]));
    fflush(stdout);
    usleep(1000000);

    for (const unsigned char *p = (const unsigned char*)argv[1]; *p; p++) {
        unsigned long ks = 0; int shift = 0;
        if (*p < 0x80) {
            char one[2] = {(char)*p, 0};
            for (size_t i = 0; i < sizeof(MAP)/sizeof(MAP[0]); i++) {
                if (MAP[i].ch[0] == (char)*p) { ks = MAP[i].keysym; shift = MAP[i].shift; break; }
            }
            (void)one;
        }
        if (!ks) { fprintf(stderr, "SKIP unmapped 0x%02x\n", *p); continue; }
        unsigned char kc = sym2code(d, ks);
        if (!kc) { fprintf(stderr, "SKIP no keycode for %c\n", *p); continue; }
        if (shift) fake_(d, shift_kc, 1, 0);
        fake_(d, kc, 1, 0);
        fake_(d, kc, 0, 0);
        if (shift) fake_(d, shift_kc, 0, 0);
        flush_(d);
        usleep(12000);
    }
    if (send_enter) {
        unsigned char ret = sym2code(d, RETURN_KS);
        fake_(d, ret, 1, 0);
        fake_(d, ret, 0, 0);
        flush_(d);
    }
    printf("done\n");
    return 0;
}
