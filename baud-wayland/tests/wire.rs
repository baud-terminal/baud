//! **The wire, byte for byte against the protocol's own layout**, and the
//! generated tables against the XMLs they were made from.

use baud_wayland::*;
use std::collections::VecDeque;

/// `wl_display.get_registry(2)`: sender 1, opcode 1, size 12, one new_id.
#[test]
fn get_registry_is_twelve_bytes_in_native_order() {
    let mut buf = [0u8; 64];
    let mut fds = Vec::new();
    let n = wire::write(
        &mut buf,
        &mut fds,
        ObjectId::DISPLAY,
        Opcode(protocol::wl_display::request::GET_REGISTRY),
        protocol::wl_display::REQUESTS[1].args,
        &[Arg::NewId(ObjectId(2))],
    )
    .unwrap();
    assert_eq!(n, 12);
    let mut want = Vec::new();
    want.extend(1u32.to_ne_bytes());
    want.extend(((12u32 << 16) | 1).to_ne_bytes());
    want.extend(2u32.to_ne_bytes());
    assert_eq!(&buf[..n], &want[..]);
    assert!(fds.is_empty());
    let h = wire::header(&buf).unwrap();
    assert_eq!(h.sender, ObjectId::DISPLAY);
    assert_eq!(h.opcode, Opcode(1));
    assert_eq!(h.size, 12);
}

/// `wl_registry.bind(3, "wl_compositor", 6, 4)`: the interface name is a
/// string -- its length with the NUL, then the bytes padded to four -- and
/// the version rides before the id.
#[test]
fn bind_carries_the_interface_name_as_a_padded_string() {
    let mut buf = [0u8; 64];
    let mut fds = Vec::new();
    let sig = protocol::wl_registry::REQUESTS[0].args;
    assert_eq!(sig, &[ArgKind::Uint, ArgKind::NewId { interface: None }]);
    let n = wire::write(
        &mut buf,
        &mut fds,
        ObjectId(2),
        Opcode(protocol::wl_registry::request::BIND),
        sig,
        &[
            Arg::Uint(3),
            Arg::NewIdWith {
                interface: "wl_compositor",
                version: 6,
                id: ObjectId(4),
            },
        ],
    )
    .unwrap();
    // 8 header + 4 name + 4 len + 16 padded string + 4 version + 4 id
    assert_eq!(n, 40);
    assert_eq!(&buf[12..16], &14u32.to_ne_bytes());
    assert_eq!(&buf[16..30], b"wl_compositor\0");
    assert_eq!(&buf[30..32], &[0, 0]);
    assert_eq!(&buf[32..36], &6u32.to_ne_bytes());
    assert_eq!(&buf[36..40], &4u32.to_ne_bytes());
    // And read back as the server would.
    let mut out = Vec::new();
    wire::read(&buf[8..n], sig, &mut VecDeque::new(), &mut out).unwrap();
    match out.as_slice() {
        [
            Read::Uint(3),
            Read::NewIdWith {
                interface: "wl_compositor",
                version: 6,
                id: ObjectId(4),
            },
        ] => {}
        other => panic!("{other:?}"),
    }
}

/// Every kind round-trips, including a null string, an empty array, a
/// fixed-point value and an fd, which takes no bytes and rides beside.
#[test]
fn every_argument_kind_round_trips() {
    let sig = [
        ArgKind::Int,
        ArgKind::Uint,
        ArgKind::Fixed,
        ArgKind::Str { nullable: true },
        ArgKind::Str { nullable: false },
        ArgKind::Object { nullable: true },
        ArgKind::Array,
        ArgKind::Fd,
        ArgKind::NewId {
            interface: Some("wl_surface"),
        },
    ];
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut buf = [0u8; 128];
    let mut fds = Vec::new();
    let n = wire::write(
        &mut buf,
        &mut fds,
        ObjectId(7),
        Opcode(3),
        &sig,
        &[
            Arg::Int(-5),
            Arg::Uint(0xdead_beef),
            Arg::Fixed(Fixed::from_f64(1.5)),
            Arg::Str(None),
            Arg::Str(Some("abc")),
            Arg::Object(ObjectId::NULL),
            Arg::Array(&[9, 8, 7]),
            Arg::Fd(std::os::fd::AsFd::as_fd(&a)),
            Arg::NewId(ObjectId(9)),
        ],
    )
    .unwrap();
    assert_eq!(fds.len(), 1);
    // 8 + 4 + 4 + 4 + 4 (null: len 0) + (4 + 4) + 4 + (4 + 4) + 0 + 4 = 48
    assert_eq!(n, 48);
    let mut got_fds = VecDeque::new();
    got_fds.push_back(std::os::fd::OwnedFd::from(a.try_clone().unwrap()));
    let mut out = Vec::new();
    wire::read(&buf[8..n], &sig, &mut got_fds, &mut out).unwrap();
    assert!(matches!(out[0], Read::Int(-5)));
    assert!(matches!(out[1], Read::Uint(0xdead_beef)));
    assert!(matches!(out[2], Read::Fixed(f) if f.to_f64() == 1.5));
    assert!(matches!(out[3], Read::Str(None)));
    assert!(matches!(out[4], Read::Str(Some("abc"))));
    assert!(matches!(out[5], Read::Object(ObjectId::NULL)));
    assert!(matches!(out[6], Read::Array(&[9, 8, 7])));
    assert!(matches!(out[7], Read::Fd(_)));
    assert!(matches!(out[8], Read::NewId(ObjectId(9))));
    assert!(got_fds.is_empty(), "the fd was taken");
}

/// The refusals: a null where none is allowed, a wrong kind, a message
/// that ends before its signature, a string without its NUL.
#[test]
fn the_wire_refuses_what_the_signature_forbids() {
    let mut buf = [0u8; 64];
    let mut fds = Vec::new();
    assert_eq!(
        wire::write(
            &mut buf,
            &mut fds,
            ObjectId(1),
            Opcode(0),
            &[ArgKind::Str { nullable: false }],
            &[Arg::Str(None)]
        ),
        Err(WireError::NullForbidden)
    );
    assert_eq!(
        wire::write(
            &mut buf,
            &mut fds,
            ObjectId(1),
            Opcode(0),
            &[ArgKind::Int],
            &[Arg::Uint(1)]
        ),
        Err(WireError::Kind)
    );
    assert_eq!(
        wire::write(
            &mut buf[..10],
            &mut fds,
            ObjectId(1),
            Opcode(0),
            &[ArgKind::Int],
            &[Arg::Int(1)]
        ),
        Err(WireError::BufferTooSmall)
    );
    let mut out = Vec::new();
    assert_eq!(
        wire::read(
            &[1, 0, 0, 0],
            &[ArgKind::Int, ArgKind::Int],
            &mut VecDeque::new(),
            &mut out
        ),
        Err(WireError::Truncated)
    );
    let bad = [4u32.to_ne_bytes(), *b"abcd"].concat();
    assert_eq!(
        wire::read(
            &bad,
            &[ArgKind::Str { nullable: false }],
            &mut VecDeque::new(),
            &mut out
        ),
        Err(WireError::BadString)
    );
    assert_eq!(
        wire::read(&[], &[ArgKind::Fd], &mut VecDeque::new(), &mut out),
        Err(WireError::MissingFd),
        "an fd the signature names has to have arrived beside the message"
    );
}

/// **The tables are the XMLs.** Sixty-three interfaces across the seventeen
/// pinned files, the opcodes the protocol documents, and the enums. The
/// primary selection's four came last, the system bell's one after them,
/// activation's two after that and xdg-foreign's four last, so no earlier
/// index moved.
#[test]
fn the_generated_tables_are_what_the_pinned_protocols_declare() {
    assert_eq!(protocol::INTERFACES.len(), 63);
    assert_eq!(protocol::zxdg_exporter_v2::INDEX, 59);
    assert_eq!(protocol::zxdg_exporter_v2::request::EXPORT_TOPLEVEL, 1);
    assert_eq!(protocol::zwp_primary_selection_device_manager_v1::INDEX, 52);
    assert_eq!(protocol::xdg_system_bell_v1::INDEX, 56);
    assert_eq!(protocol::xdg_system_bell_v1::request::RING, 1);
    assert_eq!(protocol::xdg_system_bell_v1::VERSION, 1);
    assert_eq!(protocol::xdg_activation_v1::INDEX, 57);
    assert_eq!(protocol::xdg_activation_token_v1::INDEX, 58);
    assert_eq!(
        protocol::xdg_activation_v1::request::GET_ACTIVATION_TOKEN,
        1
    );
    assert_eq!(protocol::xdg_activation_v1::request::ACTIVATE, 2);
    assert_eq!(
        protocol::xdg_activation_v1::REQUESTS[2].args,
        &[
            ArgKind::Str { nullable: false },
            ArgKind::Object { nullable: false }
        ]
    );
    assert_eq!(protocol::xdg_activation_token_v1::request::SET_SURFACE, 2);
    assert_eq!(protocol::xdg_activation_token_v1::request::COMMIT, 3);
    assert_eq!(protocol::xdg_activation_token_v1::request::DESTROY, 4);
    assert_eq!(protocol::xdg_activation_token_v1::event::DONE, 0);
    assert_eq!(
        protocol::zwp_primary_selection_offer_v1::request::RECEIVE,
        0
    );
    assert_eq!(
        protocol::zwp_primary_selection_offer_v1::REQUESTS[0].args,
        &[ArgKind::Str { nullable: false }, ArgKind::Fd]
    );
    assert_eq!(protocol::wl_data_device::request::SET_SELECTION, 1);
    assert_eq!(
        protocol::wl_data_device::REQUESTS[1].args,
        &[ArgKind::Object { nullable: true }, ArgKind::Uint]
    );
    assert_eq!(protocol::wl_display::request::SYNC, 0);
    assert_eq!(protocol::wl_display::request::GET_REGISTRY, 1);
    assert_eq!(protocol::wl_display::event::ERROR, 0);
    assert_eq!(protocol::wl_display::event::DELETE_ID, 1);
    assert_eq!(protocol::wl_registry::event::GLOBAL, 0);
    assert_eq!(protocol::wl_registry::event::GLOBAL_REMOVE, 1);
    assert_eq!(
        protocol::wl_registry::EVENTS[0].args,
        &[
            ArgKind::Uint,
            ArgKind::Str { nullable: false },
            ArgKind::Uint
        ]
    );
    assert_eq!(protocol::wl_shm::format::ARGB8888, 0);
    assert_eq!(protocol::wl_shm::format::XRGB8888, 1);
    assert_eq!(protocol::wl_shm::request::CREATE_POOL, 0);
    assert_eq!(
        protocol::wl_shm::REQUESTS[0].args,
        &[
            ArgKind::NewId {
                interface: Some("wl_shm_pool")
            },
            ArgKind::Fd,
            ArgKind::Int
        ]
    );
    assert_eq!(protocol::wl_surface::request::DESTROY, 0);
    assert!(protocol::wl_surface::REQUESTS[0].destructor);
    assert_eq!(protocol::wl_keyboard::event::KEYMAP, 0);
    assert_eq!(
        protocol::wl_keyboard::EVENTS[0].args,
        &[ArgKind::Uint, ArgKind::Fd, ArgKind::Uint]
    );
    assert_eq!(protocol::xdg_wm_base::request::GET_XDG_SURFACE, 2);
    assert_eq!(protocol::xdg_toplevel::event::CONFIGURE, 0);
    assert_eq!(protocol::wp_presentation::VERSION, 2);
    assert!(
        Interface::named("zwp_linux_dmabuf_v1").unwrap().version >= 5,
        "Hyprland requires v5"
    );
    assert!(
        Interface::named("wl_shm").unwrap().version >= 2,
        "and wl_shm v2"
    );
    assert_eq!(protocol::wp_fractional_scale_v1::event::PREFERRED_SCALE, 0);
    assert_eq!(protocol::wp_cursor_shape_device_v1::request::SET_SHAPE, 1);
    assert_eq!(protocol::zwp_text_input_v3::request::ENABLE, 1);
    assert!(Interface::named("wl_compositor").is_some());
    assert!(Interface::named("wl_teapot").is_none());
    // Every interface's opcodes are dense and every message names its args.
    for i in protocol::INTERFACES {
        assert!(!i.name.is_empty());
        for m in i.requests.iter().chain(i.events) {
            assert!(!m.name.is_empty(), "{}", i.name);
            assert!(
                m.since >= 1 && m.since <= i.version,
                "{}.{}",
                i.name,
                m.name
            );
        }
    }
}
