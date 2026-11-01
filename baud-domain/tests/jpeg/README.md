# JPEG fixtures

baud's own test pictures for the JPEG decoder, each beside the reference
decode it is held to byte for byte. The IJG's own test images are in
`baud-domain/tests/ijg/`, under the IJG's terms.

The source picture is `scene.py`'s 61 by 45 scene, written as a PPM:
gradients, a checker and a hard edge, so every sampling ratio and the
inverse DCT see detail across block edges. `small` is its 29 by 21 crop
from (11, 7), made with ImageMagick's `-crop 29x21+11+7 +repage`.

Each file was encoded by the IJG's release 6b `cjpeg`, built from
`jpegsrc.v6b.tar.gz` (sha256 75c3ec24...e35d), except `rgb.jpg`, which
6b cannot write and libjpeg-turbo 3.2.0's `cjpeg -rgb` did; every
reference was decoded by 6b's `djpeg -dct int -ppm`:

| file | `cjpeg` arguments |
|---|---|
| grey.jpg | `-grayscale` (its reference a PGM) |
| s444.jpg | `-sample 1x1` |
| s422r.jpg | `-sample 2x1 -restart 2B` |
| s420p.jpg | `-sample 2x2 -progressive` |
| s411.jpg | `-sample 4x1` |
| s440.jpg | `-sample 1x2` |
| rgb.jpg | libjpeg-turbo's `-rgb`: Adobe's `APP14`, transform 0 |
| s420seq.jpg | `-scans` with the script `0; 1; 2;`, one component a scan |
| small.jpg | no arguments, of the crop |

Two four-component files, beside ImageMagick 7.1.2's decode of each as
raw CMYK (`magick -define jpeg:dct-method=integer <file>.jpg -depth 8
cmyk:<file>.cmyk`), the samples the colour test turns into RGB:

| file | made with |
|---|---|
| cmyk.jpg | macOS ImageIO's JPEG encoder (`CGImageDestination`, quality 0.95), from a 16 by 8 DeviceCMYK bitmap whose samples are C = 15x, M = 200 - 20y, Y = 9(x + y), K = 30 + 5x: Adobe's `APP14`, transform 0 |
| ycck.jpg | `magick -size 16x8 gradient:"rgb(220,40,30)-rgb(20,90,200)" -colorspace CMYK -quality 92`: Adobe's `APP14`, transform 2, YCCK |

## Arithmetic coding, 12-bit samples and the lossless process

Encoded by libjpeg-turbo 3.1.4's `cjpeg` and decoded by its `djpeg -dct
int` (the `bin` output of nixpkgs' `libjpeg-turbo`), which decodes every
6b reference above to 6b's bytes but `s440`'s. The source is `scene.ppm`,
`scene.py`'s output, or `scene16.ppm`, `scene16.py`'s: the same scene at
16 bits, each sample its 8-bit value times 257 with a dither in its low
byte. `edge16.pgm` is `edge16.py`'s 16 by 8 grey picture whose neighbours
differ by 32768 and more. A 12-bit or 16-bit reference holds its samples
in two bytes, as `djpeg` writes them; the test draws each at its high
eight bits.

| file | source | `cjpeg` arguments | reference |
|---|---|---|---|
| a422r.jpg | scene.ppm | `-arithmetic -sample 2x1 -restart 2B` | a422r.ppm |
| a420p.jpg | scene.ppm | `-arithmetic -progressive` | a420p.ppm |
| ap444.jpg | scene.ppm | `-arithmetic -progressive -sample 1x1 -restart 3B` | ap444.ppm |
| agreyp.jpg | scene.ppm | `-arithmetic -progressive -grayscale` | agreyp.pgm |
| p12.jpg | scene16.ppm | `-precision 12` | p12.ppm |
| p12p.jpg | scene16.ppm | `-precision 12 -progressive` | p12p.ppm |
| p12a.jpg | scene16.ppm | `-precision 12 -arithmetic -restart 1` | p12a.ppm |
| p12ap.jpg | scene16.ppm | `-precision 12 -arithmetic -progressive -sample 2x1` | p12ap.ppm |
| l1.jpg to l7.jpg | scene.ppm | `-lossless N`, predictor N | scene.ppm |
| l4t2r.jpg | scene16.ppm | `-lossless 4,2 -precision 12 -restart 1` | l4t2r.ppm |
| l6p16.jpg | scene16.ppm | `-lossless 6 -precision 16` | scene16.ppm |
| l5p16t3.jpg | scene16.ppm | `-lossless 5,3 -precision 16 -restart 2` | l5p16t3.ppm |
| l7g5.jpg | grey.pgm | `-lossless 7,1 -precision 5` | l7g5.pgm |
| l2g2.jpg | grey.pgm | `-lossless 2 -precision 2 -restart 3` | l2g2.pgm |
| l1e16.jpg | edge16.pgm | `-lossless 1 -precision 16` | edge16.pgm |

`cjpeg` cannot set the arithmetic conditioning, so `dacenc.c`, compiled
against libjpeg-turbo 3.1.4 and run on `scene.ppm`, writes `adac.jpg`
(sequential) and `adacp.jpg` (progressive, its third argument given) with
DC tables at L = 2, U = 5 and L = 1, U = 3 and AC tables at Kx 20 and 40;
`adac.ppm` and `adacp.ppm` are `djpeg -dct int`'s decodes.

libjpeg-turbo writes no arithmetic-coded lossless file and ignores
`-sample` in lossless mode. Those below are Thomas Richter's libjpeg 1.71
(github.com/thorfdbg/libjpeg at 25f7128), nixpkgs' `libjpeg-tools` built
with its predictive encoder's lookup of a quantization table, which a
lossless frame never defines, answering with the table selector instead of
an error. Each writes predictor 4 and Adobe's `APP14` with transform 0,
and its own decoder returns the source of the first five. `arsub.jpg` and
its Huffman twin `lsub.jpg` halve G and B down; libjpeg-turbo's `djpeg`
decode of `lsub.jpg` is the reference both are held to.

| file | source | `jpeg` arguments | reference |
|---|---|---|---|
| ar_grey.jpg | grey.pgm | `-p -a` | grey.pgm |
| ar_rgb.jpg | scene.ppm | `-p -c -a` | scene.ppm |
| ar_rgbr.jpg | scene.ppm | `-p -c -a -z 61` | scene.ppm |
| ar16.jpg | scene16.ppm | `-p -c -a` | scene16.ppm |
| are16.jpg | edge16.pgm | `-p -a` | edge16.pgm |
| arsub.jpg | scene.ppm | `-p -c -a -s 1x1,1x2,1x2 -z 61` | lsub.ppm |
| lsub.jpg | scene.ppm | `-p -c -s 1x1,1x2,1x2 -z 61` | lsub.ppm |

libjpeg-turbo's own arithmetic test image and its Huffman twin are in
`baud-domain/tests/ijg/`: `testimgari.jpg` and `testimgint.jpg`, with
`testimgari.ppm`, its `djpeg -dct int` decode of either.
