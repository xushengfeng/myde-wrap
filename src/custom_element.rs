use smithay::backend::renderer::{
    element::{Element, Id, Kind, RenderElement},
    gles::{
        GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformValue,
    },
    utils::{CommitCounter, DamageSet, OpaqueRegions},
};
use smithay::utils::{Buffer, Physical, Rectangle, Scale, Transform};

pub struct CustomRotatedElement {
    pub id: Id,
    pub texture: GlesTexture,
    pub src: Rectangle<f64, Buffer>,
    pub dst: Rectangle<i32, Physical>,
    pub rotation: f64,
    pub shader: GlesTexProgram,
}

impl Element for CustomRotatedElement {
    fn id(&self) -> &Id {
        &self.id
    }
    fn current_commit(&self) -> CommitCounter {
        CommitCounter::default()
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.src
    }
    fn transform(&self) -> Transform {
        Transform::Normal
    }
    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.dst
    }
    fn damage_since(
        &self,
        _scale: Scale<f64>,
        _commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        return DamageSet::from_slice(&[self.dst]);
    }
    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }
    fn alpha(&self) -> f32 {
        1.0
    }
    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl RenderElement<GlesRenderer> for CustomRotatedElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let rad = self.rotation * std::f64::consts::PI / 180.0;

        let uniforms = [Uniform::new(
            "custom_rotation",
            UniformValue::_1f(rad as f32),
        )];

        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            opaque_regions,
            Transform::Normal,
            1.0,
            Some(&self.shader),
            &uniforms,
        )
    }
}
