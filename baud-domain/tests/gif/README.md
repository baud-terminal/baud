# GIF fixtures

Three GIFs written by ImageMagick 7.1.2's encoder, each beside its own
decode as raw RGBA (`magick <file>.gif -depth 8 rgba:<file>.rgba`), which
the decoder is held to byte for byte. Each image covers its logical
screen and has no transparency, so the reference holds for any decoder
that follows GIF89a's Appendix F.

| file | made with |
|---|---|
| noise256.gif | `magick -size 96x80 xc: +noise Random -colors 256 -dither None`: the string table fills and clears |
| scene64i.gif | baud's scene (`baud-domain/tests/jpeg/scene.py`) with `-colors 64 -interlace GIF` |
| scene2.gif | the scene with `-colors 2 -dither None`: the minimum code size of 2 |
