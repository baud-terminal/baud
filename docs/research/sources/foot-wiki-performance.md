# When is foot fast, and when is it not?

One of the "features" of foot is its speed, as is evident from
[benchmarks](https://codeberg.org/dnkl/foot/src/branch/master/doc/benchmark.md).

But is it _really_ fast? And if so, is it _always_ fast?

The answer to the first question is _it depends_, and the answer to
the second question is (of course) _no_.

Foot currently renders everything on the CPU. As such, it _should_
always lose out to GPU accelerated terminals, right? And it _does_, in
most setups **if** we are comparing **full grid** updates (that is,
when the entire terminal window needs to be redrawn).

Foot recently (available from 1.5.0) gained the ability to display a
render timer on-screen, which makes it easy to see how fast foot is in
real time. You can try it out for yourself by adding the snipped below
to `~/.config/foot/foot.ini`:

```ini
[tweak]
render-timer=osd
```

Here is an image that shows foot on the left, and Alacritty on the
right. I have filled both screens with `A`.

![Screenshot of terminals filled with A's](images/performance-foot-vs-alacritty-fullscreen.png)

Both shows a render timer, foot in the upper left corner, and
Alacritty in the lower left corner. This tells us how long the
**last** frame took to render.

Note: I will be using Alacritty to point out differences, and to
discuss design choices. Mainly because it is a modern,
GPU-accelerated, Wayland native terminal emulator, with at least some
goals common to foot's. And it has the ability to display a render
timer on-screen, which makes it easy to use in screenshots.

So, if foot is so slow, how come it is also fast?

The answer is twofold. The speed of a terminal emulator depends on two
things mainly: its VT parser, and its rendering engine.

Foot's VT parser is fast. _Really_ fast. Truth to be told, it is
probably the main reason foot is so fast in several of the
[vtebench](https://github.com/alacritty/vtebench) benchmarks.

I wish I could take all the credit for this, but alas, I have had help
from the compiler; building foot with [profile guided
optimizations](https://codeberg.org/dnkl/foot/src/branch/master/INSTALL.md#user-content-profile-guided-optimization)
boosts performance noticeably. But the VT parser's speed is _also_ the
result of doing performance analyses and tweaking the code. Here it
seems there is always room for improvement.

The rendering engine in foot is, as we have just seen, _not_ faster
than lightning. However, most of the time, applications do **not**
update the entire screen. If you are typing things on the command
line, or editing a text file, only a couple of characters are updated
each time.

Foot makes use of this information, and only renders the cells that
have been updated. This is called _damage tracking_.

Here is another screenshot. Again, foot is on the left and Alacritty
on the right.

![Screeshot of terminals where 'f' was just pressed](images/performance-foot-vs-alacritty-single-cell.png)

It looks almost the same as the previous screenshot. The difference
is, in the first screenshot I had scrolled up and down in the
scrollback history using <kbd>Page</kbd>+<kbd>Up</kbd>, to force a
full grid refresh, while in this screenshot I just pressed
<kbd>f</kbd> on the prompt.

As can be seen, there is no difference in the render time for
Alacritty, while foot is significantly faster compared to the first
screenshot.

This is because Alacritty does not do damage tracking; it always
re-renders the entire grid.

Yet another screenshot:

![Screenshot of terminals with almost empty grids](images/performance-foot-vs-alacritty-fullscreen-but-empty.png)

Both are fast! What happened? Did not Alacritty re-render the entire
grid, every frame? Yes, it does, but it also is **much** faster at
rendering _empty_ cells than cells with glyphs in them.

In other words, in this screenshot, both terminals are fast, but for
different reasons; Alacritty renders empty cells _really_ fast, and
foot does damage tracking and does not render anything but the last
updated cells.

There is yet one more important thing to consider in the rendering
engine: scrolling. By scrolling, I mean both scrolling up the content
when you are at the bottom of the screen and e.g. presses
<kbd>return</kbd>, and when scrolling up or down in the scrollback
history.

Technically, scrolling results in a full grid update; _every_ cell in
the grid has changed.

However, by being clever, foot manages to render this _without_
re-rendering the entire grid. Note that this is only true as long as
some of the original content remains on the screen. If you scroll a
page or more, there is no magic to be had, and foot is forced to
re-render the entire grid.

Foot implements two algorithms for _scroll damage_ (my name for it;
custom damage tracking for scrolling).

The first one is a simpler variant that takes the pixmap of the last
rendered frame, and _moves_ the content using a single
`memmove()`. This is fast when scrolling a large number of lines since
then the amount of data to move is small. On the other hand, it is
fairly slow when scrolling a small number of lines since then it needs
to move almost all of the old content.

On 64-bit platforms, where the amount of **virtual** address space is
large, it implements a memory mapping trick to avoid large memory
moves. You can read more about it in the _TWEAK_ section in the
`foot.ini(5)` man page.

This screenshot illustrates this. It is, once again, a screen full of
`A`s, where I have scrolled up in the scrollback history a couple of
lines only, using the mouse.

![Screenshot of terminals filled with A's, scrolled up](images/performance-foot-vs-alacritty-scrolling.png)

Once again, Alacritty is neither faster nor slower than before. Foot
on the other hand is slightly slower than in the last screenshot,
where only a couple of cells had changed, but still **much** faster
than in the first screenshot where it were forced to re-render the
full grid.

To summarize, foot is designed and optimized for interactive
use. While it often **is** very fast in benchmarks too, this can
mostly be attributed to its VT parser, which makes it handle "dumb"
benchmarks, that just push lots of data to the terminal, very well.

Finally, it should be mentioned that while I was writing this, and
taking screenshots, **both** terminal emulators had varying render
timings. Most likely because I was doing this on a laptop in battery
mode. I did try to pick timings that I believe are "normal". And, the
point of this article is **not** to _compare_ performance, but to
point out what and how foot does things, and how it affects its
performance in different situations.

One last thing: I encourage you to try the `notcurses-demo` from
https://github.com/dankamongmen/notcurses. And do try it with the
on-screen render-timer enabled!
