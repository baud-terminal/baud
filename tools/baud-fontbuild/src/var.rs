//! The positions variation data is resolved at, and the formatting of
//! what it resolves to.

use std::collections::BTreeSet;

use read_fonts::tables::layout::DeviceOrVariationIndex;
use read_fonts::tables::variations::{DeltaSetIndex, ItemVariationStore};
use read_fonts::types::{F2Dot14, Fixed};
use read_fonts::{FontRef, TableProvider};

/// Normalized positions, the same list for both fonts: built from the
/// union of what either names, so a sample never differs only because
/// one font has a region the other lacks.
pub struct Samples {
    pub coords: Vec<Vec<F2Dot14>>,
}

fn store_regions(store: &ItemVariationStore, axes: &mut [BTreeSet<i16>]) {
    let Ok(list) = store.variation_region_list() else {
        return;
    };
    for region in list.variation_regions().iter().flatten() {
        for (i, axis) in region.region_axes().iter().enumerate() {
            if let Some(set) = axes.get_mut(i) {
                for c in [axis.start_coord(), axis.peak_coord(), axis.end_coord()] {
                    set.insert(c.to_bits());
                }
            }
        }
    }
}

impl Samples {
    pub fn of(a: &FontRef, b: &FontRef) -> Samples {
        let count = |f: &FontRef| f.fvar().map_or(0, |v| v.axis_count() as usize);
        let n = count(a).max(count(b));
        let mut axes = vec![BTreeSet::from([-0x4000_i16, 0, 0x4000]); n];
        for font in [a, b] {
            if let Ok(gdef) = font.gdef()
                && let Some(Ok(store)) = gdef.item_var_store()
            {
                store_regions(&store, &mut axes);
            }
            if let Ok(hvar) = font.hvar()
                && let Ok(store) = hvar.item_variation_store()
            {
                store_regions(&store, &mut axes);
            }
        }
        let mut coords = vec![vec![F2Dot14::ZERO; n]];
        for (i, set) in axes.iter().enumerate() {
            for &v in set.iter().filter(|&&v| v != 0) {
                let mut c = vec![F2Dot14::ZERO; n];
                c[i] = F2Dot14::from_bits(v);
                coords.push(c);
            }
        }
        if n > 1 {
            coords.push(vec![F2Dot14::from_bits(-0x4000); n]);
            coords.push(vec![F2Dot14::from_bits(0x4000); n]);
        }
        Samples { coords }
    }

    /// One delta-set index resolved at every sample, `{d0,d1,..}`.
    pub fn resolve(&self, store: Option<&ItemVariationStore>, index: DeltaSetIndex) -> String {
        let Some(store) = store else {
            return format!("{{no store for {}:{}}}", index.outer, index.inner);
        };
        let deltas: Vec<String> = self
            .coords
            .iter()
            .map(|c| match store.compute_delta(index, c) {
                Some(d) => num(d.to_bits() as f64 / 65536.0),
                None => "err".to_string(),
            })
            .collect();
        format!("{{{}}}", deltas.join(","))
    }

    /// A device or variation-index table: a hinting device's deltas as
    /// stored, a variation index resolved.
    pub fn device(
        &self,
        store: Option<&ItemVariationStore>,
        dev: Option<Result<DeviceOrVariationIndex, read_fonts::ReadError>>,
    ) -> String {
        match dev {
            None => String::new(),
            Some(Err(e)) => format!("dev(err:{e})"),
            Some(Ok(DeviceOrVariationIndex::Device(d))) => {
                let v: Vec<String> = d.iter().map(|x| x.to_string()).collect();
                format!("dev({}-{}:{})", d.start_size(), d.end_size(), v.join(","))
            }
            Some(Ok(DeviceOrVariationIndex::VariationIndex(v))) => {
                let idx = DeltaSetIndex {
                    outer: v.delta_set_outer_index(),
                    inner: v.delta_set_inner_index(),
                };
                format!("var{}", self.resolve(store, idx))
            }
        }
    }
}

/// A number as Rust's shortest round-trip form, integers without `.0`.
pub fn num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

pub fn f2(v: F2Dot14) -> String {
    num(v.to_f32() as f64)
}

pub fn fx(v: Fixed) -> String {
    num(v.to_bits() as f64 / 65536.0)
}
