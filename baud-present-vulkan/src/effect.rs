//! **An effect trail drawn by the instance path**: the effect's records
//! (`baud_render::effect::Vfx::instances`) by its own fragment shader
//! ([`Fx`]), each blended over what the target holds, with the effect's
//! `EffectU` (`Vfx::uniform`, 144 bytes) bound at set 2.

use crate::draw::{Pass, Scene};
use crate::frame::{Failed, Gpu, Multiply, Target};
use crate::pipeline::{Blend, Fragment, Fx, Pipelines};

/// **Whether an effect was drawn** by [`Gpu::draw_effect_when_ready`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    Yes,
    /// A pipeline it needs is still building: nothing drawn this frame,
    /// and nothing waited for ([`Pipelines::not_ready`] counts it).
    Building,
}

impl Gpu {
    /// The passes `fx` by `blend` is drawn in on this device: one, or the
    /// two of [`Blend::Multiply`] where there is no second source.
    fn passes(&self, fx: Fx, blend: Blend) -> Vec<(Fx, Blend)> {
        match (blend, self.multiply) {
            (Blend::Multiply, Multiply::TwoPass) => vec![
                (Fx::PlasmaFactor, Blend::MultiplyFactor),
                (Fx::Plasma, Blend::MultiplyAdd),
            ],
            // The second source is the multiply shader's own output: no
            // other stage writes it.
            (Blend::Multiply, Multiply::DualSource) => vec![(Fx::PlasmaMultiply, blend)],
            _ => vec![(fx, blend)],
        }
    }

    /// **An effect's records drawn over `t`** by `fx` and `blend`, with
    /// `effect_u` its uniform: the target's pixels kept and blended over
    /// ([`Pass::Over`]), the scene's records and uniform as
    /// [`Gpu::draw`] reads them. [`Blend::Multiply`] on a device without a
    /// second source ([`Multiply::TwoPass`]) is two passes -- the factor
    /// ([`Fx::PlasmaFactor`]) multiplied into the backdrop, its alpha kept,
    /// then the plasma's colour times `1 - ab` added -- exact where the
    /// records do not overlap, and counted in
    /// [`Pipelines::multiply_two_pass`]. Builds a pipeline it lacks, and
    /// waits for it: for a frame that may wait, a test's.
    pub fn draw_effect(
        &self,
        pipes: &mut Pipelines,
        t: &Target,
        scene: &Scene<'_>,
        fx: Fx,
        blend: Blend,
        effect_u: &[u8; 144],
    ) -> Result<(), Failed> {
        let passes = self.passes(fx, blend);
        if passes.len() > 1 {
            pipes.multiply_two_pass.0 += 1;
        }
        for (fx, blend) in passes {
            self.draw_with(
                pipes,
                t,
                scene,
                Pass::Over,
                (Fragment::Effect(fx), blend),
                Some(&effect_u[..]),
            )?;
        }
        Ok(())
    }

    /// **[`Gpu::draw_effect`] if its pipelines are built, and nothing if
    /// they are not**, never waiting: a pipeline it lacks starts building
    /// on the worker ([`Pipelines::ready`]), whose finish makes
    /// [`Pipelines::ready_fd`] readable, so the loop owes the frame that
    /// draws it. What the frame draws while the effect's pipeline builds is
    /// the frame without the effect.
    pub fn draw_effect_when_ready(
        &self,
        pipes: &mut Pipelines,
        t: &Target,
        scene: &Scene<'_>,
        fx: Fx,
        blend: Blend,
        effect_u: &[u8; 144],
    ) -> Result<Drawn, Failed> {
        let missing = self
            .passes(fx, blend)
            .into_iter()
            .filter(|&(fx, blend)| pipes.ready(Fragment::Effect(fx), blend).is_none())
            .count();
        if missing > 0 {
            return Ok(Drawn::Building);
        }
        self.draw_effect(pipes, t, scene, fx, blend, effect_u)?;
        Ok(Drawn::Yes)
    }

    /// **The fireworks' stars drawn over `t`**, each record a star by
    /// [`Fragment::Fireworks`], source-over, with `fw_u` its uniform
    /// (`fireworks::uniform`: where the frame sits, the hole's centre and
    /// radius). Never waits: nothing is drawn while its pipeline builds.
    pub fn draw_fireworks(
        &self,
        pipes: &mut Pipelines,
        t: &Target,
        scene: &Scene<'_>,
        fw_u: &[u8; 32],
    ) -> Result<Drawn, Failed> {
        if pipes.ready(Fragment::Fireworks, Blend::Normal).is_none() {
            return Ok(Drawn::Building);
        }
        self.draw_with(
            pipes,
            t,
            scene,
            Pass::Over,
            (Fragment::Fireworks, Blend::Normal),
            Some(&fw_u[..]),
        )?;
        Ok(Drawn::Yes)
    }
}
