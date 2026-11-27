/* Encode a PPM with arithmetic coding and conditioning other than the
 * defaults: DC table 0 L=2 U=5, DC table 1 L=1 U=3, AC table 0 Kx=20,
 * AC table 1 Kx=40; progressive when a third argument is given. */
#include <stdio.h>
#include <stdlib.h>
#include <jpeglib.h>
int main(int argc, char **argv) {
  FILE *in = fopen(argv[1], "rb"), *out = fopen(argv[2], "wb");
  int w, h, max;
  if (!in || !out || fscanf(in, "P6 %d %d %d", &w, &h, &max) != 3) return 1;
  fgetc(in);
  unsigned char *px = malloc((size_t)w * h * 3);
  if (fread(px, 3, (size_t)w * h, in) != (size_t)w * h) return 1;
  struct jpeg_compress_struct c;
  struct jpeg_error_mgr e;
  c.err = jpeg_std_error(&e);
  jpeg_create_compress(&c);
  jpeg_stdio_dest(&c, out);
  c.image_width = w; c.image_height = h;
  c.input_components = 3; c.in_color_space = JCS_RGB;
  jpeg_set_defaults(&c);
  c.arith_code = TRUE;
  c.dct_method = JDCT_ISLOW;
  c.arith_dc_L[0] = 2; c.arith_dc_U[0] = 5; c.arith_ac_K[0] = 20;
  c.arith_dc_L[1] = 1; c.arith_dc_U[1] = 3; c.arith_ac_K[1] = 40;
  if (argc > 3) jpeg_simple_progression(&c);
  jpeg_start_compress(&c, TRUE);
  while (c.next_scanline < c.image_height) {
    JSAMPROW row = px + (size_t)c.next_scanline * w * 3;
    jpeg_write_scanlines(&c, &row, 1);
  }
  jpeg_finish_compress(&c);
  jpeg_destroy_compress(&c);
  fclose(out);
  return 0;
}
