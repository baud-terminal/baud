// macOS's own colour conversion, the oracle baud-domain/tests/themeiterm.rs
// holds the iTerm2 reader to: each colour on a 7x7x7 grid of components, in
// calibrated RGB, Display P3 and sRGB, converted by NSColor to sRGB.
// Run on a Mac: osascript -l JavaScript scripts/nscolor-oracle.js
ObjC.import('AppKit');
function conv(space, r, g, b) {
  var c;
  if (space === 'cal') c = $.NSColor.colorWithCalibratedRedGreenBlueAlpha(r, g, b, 1);
  else if (space === 'p3') c = $.NSColor.colorWithDisplayP3RedGreenBlueAlpha(r, g, b, 1);
  else c = $.NSColor.colorWithSRGBRedGreenBlueAlpha(r, g, b, 1);
  var s = c.colorUsingColorSpace($.NSColorSpace.sRGBColorSpace);
  return [s.redComponent, s.greenComponent, s.blueComponent];
}
var out = [];
// Steps off the grid where a result lands on a half step (0.5 * 255 =
// 127.5), where single and double precision round apart.
var vals = [0, 0.07, 0.23, 0.41, 0.66, 0.88, 1];
['cal', 'p3', 'srgb'].forEach(function (sp) {
  vals.forEach(function (r) { vals.forEach(function (g) { vals.forEach(function (b) {
    var v = conv(sp, r, g, b);
    out.push(sp + ' ' + r + ' ' + g + ' ' + b + ' ' + v.map(function (x) { return x.toFixed(9); }).join(' '));
  }); }); });
});
out.join('\n');
