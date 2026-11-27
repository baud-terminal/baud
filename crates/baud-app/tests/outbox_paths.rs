//! **Every byte owed to the child goes through the master's room**
//! (`pty::OutboxWatch`), held in the source because no test opens a window.
//!
//! A write the kernel took in part leaves bytes owed, and only
//! `OutboxWatch::write` registers the master's room for them: a write that
//! went round it would leave them owed with nothing registered, and a still
//! window whose child blocks reading the rest would wait for ever, the
//! child with it. And the watch is dropped before the master closes only
//! because `struct App` declares `room` ahead of `pty`: fields drop in
//! declaration order, so the other order closes the descriptor its
//! dispatch source is still registered on.
//!
//! Each check is a function of the source, run on `app.rs` as it is and on
//! a copy carrying its perturbation, which it must refuse: a check that
//! cannot fire fails here instead of passing.

/// The driver, as compiled into the app: every file of its source, in the
/// order of `APP_SOURCE` in `src/app_source.rs`.
const APP: &str = concat!(include_str!("../src/app.rs"));

/// The one write of what the child is owed, in `App::flush_reply`.
const THE_WRITE: &str = "self.room.write(&self.pty, &mut self.outbox)";

/// `src` less every item compiled for tests alone: each `#[cfg(test)]` or
/// `#[cfg(all(test, ...))]` at the left margin and the item under it, to
/// its closing brace at the margin, or to its `;` when it has no body
/// (`mod key_repeat;`).
fn outside_tests(src: &str) -> String {
    let mut kept = String::with_capacity(src.len());
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        let test_only = line.starts_with("#[cfg(test)]") || line.starts_with("#[cfg(all(test,");
        if !test_only {
            kept.push_str(line);
            kept.push('\n');
            continue;
        }
        // The item's lines: attributes, then its head, then its body to its
        // closing brace, unless the head ends the item.
        let mut head = true;
        for item in lines.by_ref() {
            if item.starts_with("#[") {
                continue;
            }
            if (head && item.ends_with(';')) || item == "}" {
                break;
            }
            head = false;
        }
    }
    kept
}

/// Whether the driver writes to the child only through the master's room:
/// the one write is there, and no call names the pty's write or the
/// descriptor's.
fn writes_only_through_the_room(src: &str) -> Result<(), String> {
    let code = outside_tests(src);
    let writes = code.matches(THE_WRITE).count();
    if writes != 1 {
        return Err(format!("{writes} writes through the room, not one"));
    }
    for around in ["pty.write(", "Pty::write(", "write_some("] {
        if let Some(at) = code.find(around) {
            let line = code[..at].lines().count();
            return Err(format!(
                "`{around}` outside the tests, line {line} of what is kept"
            ));
        }
    }
    Ok(())
}

/// The field declared at the left margin of `struct App` as `    <name>:`,
/// by its position in the struct.
fn field_at(body: &str, name: &str) -> Option<usize> {
    body.lines().position(|l| {
        l.strip_prefix("    ")
            .and_then(|l| l.strip_prefix(name))
            .is_some_and(|l| l.starts_with(": "))
    })
}

/// Whether `struct App` declares `room` ahead of `pty`, so the watch drops
/// first.
fn room_drops_before_pty(src: &str) -> Result<(), String> {
    let body = src
        .split_once("\nstruct App {\n")
        .and_then(|(_, rest)| rest.split_once("\n}\n"))
        .ok_or("no `struct App`")?
        .0;
    let room = field_at(body, "room").ok_or("`struct App` has no `room`")?;
    let pty = field_at(body, "pty").ok_or("`struct App` has no `pty`")?;
    match room < pty {
        true => Ok(()),
        false => Err(format!("`room` is field {room}, after `pty` at {pty}")),
    }
}

/// `src` with `from` replaced once, which must be there: a perturbation
/// whose anchor moved would perturb nothing.
fn perturbed(src: &str, from: &str, to: &str) -> String {
    assert_eq!(src.matches(from).count(), 1, "the anchor `{from}`");
    src.replacen(from, to, 1)
}

/// **No path writes the outbox round the master's room.** `app.rs`, less
/// its tests, writes what is owed once, through `room.write`, and names
/// neither `Pty::write` nor `write_some`.
///
/// Perturbation: `flush_reply` writing through `self.pty.write(` instead,
/// which leaves a short write's rest with nothing registered; refused.
#[test]
fn every_outbox_write_goes_through_the_room() {
    assert_eq!(writes_only_through_the_room(APP), Ok(()));
    let around = perturbed(APP, THE_WRITE, "self.pty.write(&mut self.outbox)");
    assert!(
        writes_only_through_the_room(&around).is_err(),
        "the perturbation is refused"
    );
    // A test module's mention is not a write: the same call, in a test, is kept out.
    let in_a_test =
        format!("{APP}\n#[cfg(test)]\nmod extra {{\n    fn f() {{ pty.write_some(); }}\n}}\n");
    assert_eq!(writes_only_through_the_room(&in_a_test), Ok(()));
}

/// **The watch is dropped before the master closes**: `struct App`
/// declares `room` ahead of `pty`.
///
/// Perturbation: the two declarations swapped, so `pty` drops first and
/// closes the master under a registered source; refused.
#[test]
fn the_room_is_declared_before_the_pty() {
    assert_eq!(room_drops_before_pty(APP), Ok(()));
    let swapped = perturbed(
        APP,
        "    room: crate::pty::OutboxWatch,\n    pty: Pty,\n",
        "    pty: Pty,\n    room: crate::pty::OutboxWatch,\n",
    );
    assert!(
        room_drops_before_pty(&swapped).is_err(),
        "the perturbation is refused"
    );
}
