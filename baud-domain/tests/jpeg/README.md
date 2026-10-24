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
