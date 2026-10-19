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
    /// `> 4 m` -- XTQMODKEYS's string, `modifyOtherKeys` (`[[source:
    /// ctlseqs]]`'s list, "> Pm m -> XTQMODKEYS (xterm)").
    OtherKeys,
    /// `> 4 f` -- XTQFMTKEYS's string, `formatOtherKeys`.
    OtherKeysFormat,
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
            [b'>', rest @ ..] => StatusTarget::key_option(rest),
            _ => StatusTarget::Unknown,
        }
    }

    /// `Pm` and a final after the `>`: the resource baud holds, `4`, with
    /// `m` or `f`. Any other resource is one baud does not act on, so
    /// [`StatusTarget::Unknown`], answered invalid.
    fn key_option(rest: &[u8]) -> StatusTarget {
        let Some((&last, digits)) = rest.split_last() else {
            return StatusTarget::Unknown;
        };
        let mut pm = Param::ZERO;
        for &d in digits {
            match d {
                b'0'..=b'9' => pm = pm.push(Digit::of_value(d - b'0')),
                _ => return StatusTarget::Unknown,
            }
        }
        match (digits.is_empty(), pm.value(), last) {
            (false, 4, b'm') => StatusTarget::OtherKeys,
            (false, 4, b'f') => StatusTarget::OtherKeysFormat,
            _ => StatusTarget::Unknown,
        }
    }
}
