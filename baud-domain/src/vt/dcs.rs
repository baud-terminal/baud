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
    /// `SP t` -- DECSWBV, the warning bell's volume, and `SP u` -- DECSMBV,
    /// the margin bell's: the VT520's DECRQSS list names both (`[[source:
    /// vt520-rm]]` pp. 5-103 and 5-104), where ctlseqs' list does not.
    WarningBellVolume,
    MarginBellVolume,
    /// `" q` — DECSCA.
    Protection,
    /// `$ |` — DECSCPP, the columns per page.
    ColumnsPerPage,
    /// `* x` — DECSACE, the attribute change extent: "Select attribute change
    /// extent DECSACE *x" in `[[source: vt420-rm]]` Table 12-4 and in the
    /// VT510's DECRQSS list.
    AttrExtent,
    /// `> Pp m` -- XTQMODKEYS's string, a key-modifier resource
    /// (`[[source: ctlseqs]]`'s list, "> Pm m -> XTQMODKEYS (xterm)").
    KeyModify(KeyResource),
    /// `> Pp f` -- XTQFMTKEYS's string, its format.
    KeyFormat(KeyResource),
    /// `" p` -- DECSCL, the conformance level and the C1 transmission.
    ConformanceLevel,
    /// `> t` or `> Ps t` -- XTSMTITLE's string (`[[source: ctlseqs]]`'s
    /// list, "> Pm t -> XTSMTITLE (xterm)"), the title modes. `Ps` names
    /// one of the four, as xterm's `do_dcs` reads it; the answer is every
    /// mode set either way.
    TitleModes,
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
            b" t" => StatusTarget::WarningBellVolume,
            b" u" => StatusTarget::MarginBellVolume,
            b"\"q" => StatusTarget::Protection,
            b"$|" => StatusTarget::ColumnsPerPage,
            b"*x" => StatusTarget::AttrExtent,
            b"\"p" => StatusTarget::ConformanceLevel,
            [b'>', rest @ ..] => StatusTarget::key_option(rest),
            _ => StatusTarget::Unknown,
        }
    }

    /// `Pm` and a final after the `>`: a resource with `m` or `f`, or the
    /// title modes with `t`, `Pm` omitted or one of the four. A `Pm` naming
    /// none is [`StatusTarget::Unknown`], answered invalid, as xterm's
    /// `do_dcs` answers it.
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
        match (digits.is_empty(), KeyResource::from_param(pm.value()), last) {
            (false, Some(r), b'm') => StatusTarget::KeyModify(r),
            (false, Some(r), b'f') => StatusTarget::KeyFormat(r),
            (true, _, b't') => StatusTarget::TitleModes,
            (false, _, b't') if TitleMode::from_param(pm).is_some() => StatusTarget::TitleModes,
            _ => StatusTarget::Unknown,
        }
    }
}
