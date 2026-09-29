# Theme file fixtures

Hand-made, for `baud-app/src/palette.rs`'s tests; none is from a published
theme collection.

- `day.yaml`: a whole base16 light scheme in the current nested form,
  declared `light`.
- `sepia.yml`: the same colours declaring `variant: "dark"`, so the declared
  variant disagrees with the page and text; and the `.yml` extension.
- `broken.yaml`: `day.yaml` without `base0F`, which the base16 reader refuses.
- `night`: a partial ghostty theme with no extension -- the eight normals,
  entry 42, the page, the text, and one key baud does not read.
