use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Angle, AngleUnit, DrawTarget, Point, Primitive},
    primitives::{Arc, PrimitiveStyle},
    Drawable
};

pub struct ProgressBar {
    maximum_value: f32,
    origin: Point,
    style: PrimitiveStyle<BinaryColor>,
}

impl ProgressBar {
    const DIAMETER: u32 = 10;

    pub fn new(origin: Point, max: impl Into<f32>) -> Self {
        let style = PrimitiveStyle::with_stroke(BinaryColor::On, 2);
        Self {
            maximum_value: max.into(),
            origin,
            style,
        }
    }

    fn calculate_progress(&self, current: f32) -> Angle {
        (((current / self.maximum_value) * 360.0) * -1.0).deg()
    }

    pub fn draw_at<T: DrawTarget<Color = BinaryColor>>(
        &self,
        current: impl Into<f32>,
        target: &mut T,
    ) -> Result<(), <T as DrawTarget>::Error> {
        let progress = self.calculate_progress(current.into());
        Arc::new(self.origin, Self::DIAMETER, 90.0_f32.deg(), progress)
            .into_styled(self.style)
            .draw(target)?;
        Ok(())
    }
}
