//! The compositor: `Term::viewport` in, pixels out.
//!
//! **The shape every shipping GPU terminal shares**, and the one measured
//! bottleneck among them (`docs/research/notes/j-rendering.md` D2): the CPU
//! turns cells into flat fixed-size instance records, one memcpy puts them in
//! driver-visible memory, one or two instanced draws expand them into
//! atlas-textured quads. The GPU is never the bound; the conversion is. So
//! the conversion is built first, on the CPU, where `baud-bench`'s `frame`
//! harness can price it without a GPU.
//!
//! Damage is row-granular and lives outside the cell (D12): `CellFlags` is
//! fully allocated. Placement of the instance buffer is a per-architecture
//! startup decision (D3), and nothing here maps the live grid as a GPU buffer
//! (D4).
//!
//! Empty until phase 4 of the approved plan; scoped into every gate first.
#![forbid(unsafe_code)]
