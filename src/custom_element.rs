use smithay::backend::renderer::{
    element::{Element, Id, Kind, RenderElement},
    gles::{
        GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformValue,
    },
    utils::{CommitCounter, DamageSet, OpaqueRegions},
};
use smithay::utils::{Buffer, Physical, Rectangle, Scale, Transform};

use crate::protocol::Rect;

/// 旋转截取 + 拉伸填充元素
///
/// 从画布纹理中截取 `region`（画布坐标：左上角 (x, y) + 长宽 (w, h)）：
/// 区域以 (x, y) 为锚点、绕锚点旋转 `rotation` 度，截取结果转正后拉伸填充到 `dst`
///（一般为整个屏幕），实现隐式缩放（scale = dst 尺寸 / 截取长宽）并保持直角。
/// 等价于把画布绕 (x, y) 旋转 -`rotation` 度后，截取以锚点为左上角的 w×h 区域，
/// 拉伸铺满 `dst`。
pub struct CropStretchElement {
    pub id: Id,
    pub texture: GlesTexture,
    /// 整幅画布纹理（画布像素坐标）
    pub src: Rectangle<f64, Buffer>,
    /// 截取区域（画布像素坐标）
    pub region: Rect,
    /// 截取区域相对画布的旋转角（度）
    pub rotation: f64,
    /// 目标区域（物理坐标）
    pub dst: Rectangle<i32, Physical>,
    pub shader: GlesTexProgram,
}

impl Element for CropStretchElement {
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
        DamageSet::from_slice(&[self.dst])
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

impl RenderElement<GlesRenderer> for CropStretchElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let rad = (self.rotation * std::f64::consts::PI / 180.0) as f32;
        let uniforms = [
            Uniform::new(
                "u_region",
                UniformValue::_4f(
                    self.region.x as f32,
                    self.region.y as f32,
                    self.region.width as f32,
                    self.region.height as f32,
                ),
            ),
            Uniform::new("u_rotation", UniformValue::_1f(rad)),
            Uniform::new(
                "u_canvas",
                UniformValue::_2f(self.src.size.w as f32, self.src.size.h as f32),
            ),
        ];

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
