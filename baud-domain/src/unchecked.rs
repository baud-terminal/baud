//! Left as runtime checks, deliberately.
//!
//! * **Viewport reconstructibility.** That materialising from
//!   `(lines, cols, anchor)` reproduces the same cells is a property of the
//!   materialiser, not of any type. Assert it in debug builds — if it fails,
//!   state is living only in the projection, which is the promoted-line bug.
//! * **`rows[i] == ceil(width[i] / cols)`.** Derived, so it is cheap to
//!   recompute and compare. A drift here means the index and the text
//!   disagree.
//! * **Alt-screen lines never entering scrollback.** Expressible as another
//!   typestate, but it would infect every call site for one rule.
//! * **The 245 us fallback resolution never happening on the frame thread.**
//!   A thread-affinity property. Enforce with a debug assertion on thread id
//!   inside the resolver.
