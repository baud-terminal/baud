#include <stdio.h>
#include <X11/Xlib.h>
int main(int argc, char **argv) {
  Display *d = XOpenDisplay(NULL);
  if (!d) { fprintf(stderr, "no display\n"); return 2; }
  Colormap cm = DefaultColormap(d, DefaultScreen(d));
  Visual *v = DefaultVisual(d, DefaultScreen(d));
  fprintf(stderr, "depth %d bits_per_rgb %d class %d\n", DefaultDepth(d, DefaultScreen(d)), v->bits_per_rgb, v->class);
  char line[512];
  while (fgets(line, sizeof line, stdin)) {
    size_t n = 0; while (line[n] && line[n] != '\n') n++; line[n] = 0;
    XColor c = {0};
    if (!XParseColor(d, cm, line, &c)) { printf("%s\tFAIL\n", line); continue; }
    XColor a = c;
    int ok = XAllocColor(d, cm, &a);
    printf("%s\t%04x/%04x/%04x\t%s%06lx\t%04x/%04x/%04x\n", line, c.red, c.green, c.blue, ok ? "" : "noalloc:", a.pixel, a.red, a.green, a.blue);
  }
  return 0;
}
