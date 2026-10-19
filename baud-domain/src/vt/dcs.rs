use crate::*;

/// A DCS (Device Control String), resolved from its parameters and final byte.
///
/// `[[source: ctlseqs]]` gives `DCS $ q Pt ST` as DECRQSS, "Request Status
/// String, VT420 and up", where `Pt` names a setting by the tail of the CSI
/// that would set it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dcs {
    /// DECRQSS, `DCS $ q`. The payload names which setting is being asked
    /// about; see [`StatusTarget`].
    RequestStatus,
    /// Recognised as a DCS and deliberately not acted on. The payload is still
    /// consumed, which is the part that matters for safety.
    Unsupported,
}

impl Dcs {
    pub fn parse(inter: Option<StreamByte>, f: FinalByte) -> Dcs {
        match (inter.map(|i| i.raw_byte()), f.raw()) {
            (Some(b'$'), b'q') => Dcs::RequestStatus,
            _ => Dcs::Unsupported,
        }
    }
}

/// Which setting a DECRQSS asks about.
///
/// `[[source: ctlseqs]]` lists the request strings: "m -> SGR, \" p -> DECSCL,
/// SP q -> DECSCUSR, \" q -> DECSCA, r -> DECSTBM, s -> DECSLRM, ...". baud
/// answers for the settings it actually has; everything else is
/// [`StatusTarget::Unknown`] and gets the invalid reply, which is the honest
/// answer rather than a fallback.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusTarget {
    /// `m` — the current SGR.
    Sgr,
    /// `r` — DECSTBM, the top and bottom margins.
    ScrollRegion,
    /// `s` — DECSLRM, the left and right margins.
    SideMargins,
    /// `SP q` — DECSCUSR.
    CursorStyle,
    /// `" q` — DECSCA.
    Protection,
    /// `$ |` — DECSCPP, the columns per page.
    ColumnsPerPage,
    /// `* x` — DECSACE, the attribute change extent: "Select attribute change
    /// extent DECSACE *x" in `[[source: vt420-rm]]` Table 12-4 and in the
    /// VT510's DECRQSS list.
    AttrExtent,
    Unknown,
}

impl StatusTarget {
    /// Resolve the `Pt` of a DECRQSS.
    pub fn parse(pt: &[u8]) -> StatusTarget {
        match pt {
            b"m" => StatusTarget::Sgr,
            b"r" => StatusTarget::ScrollRegion,
            b"s" => StatusTarget::SideMargins,
            b" q" => StatusTarget::CursorStyle,
            b"\"q" => StatusTarget::Protection,
            b"$|" => StatusTarget::ColumnsPerPage,
            b"*x" => StatusTarget::AttrExtent,
            _ => StatusTarget::Unknown,
        }
    }
}
