#!/bin/bash
# Note AJ, sections 2.3 and 3.1: two searches with positive controls.
# Run from the repository root.
echo "== assets/logo/*.svg: files with animation markup, and the control"
echo "animate|@keyframes|<style: $(grep -l -E 'animate|@keyframes|<style' assets/logo/*.svg | wc -l | tr -d ' ') of $(ls assets/logo/*.svg | wc -l | tr -d ' ') files"
echo "rect (control): $(grep -l 'rect' assets/logo/*.svg | wc -l | tr -d ' ') of $(ls assets/logo/*.svg | wc -l | tr -d ' ') files"
echo "== docs/research/sources/sfte.h: matching lines"
f=docs/research/sources/sfte.h
echo "splash, case-insensitive: $(grep -c -i 'splash' $f)"
echo "SFTE_TERM_ANIMATE_SCREEN (control): $(grep -c 'SFTE_TERM_ANIMATE_SCREEN' $f)"
echo "logo, case-sensitive: $(grep -n 'logo' $f | cut -d: -f1 | tr '\n' ' ')"
echo "logo, case-insensitive: $(grep -n -i 'logo' $f | cut -d: -f1 | tr '\n' ' ')"
echo "logo, whole word, case-insensitive: $(grep -c -i -w 'logo' $f)"
