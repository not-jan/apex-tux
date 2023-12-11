use anyhow::Result;
use apex_hardware::BitVec;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{iso_8859_15::FONT_6X10, MonoFont, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    text::{renderer::TextRenderer, Baseline, Text},
    Drawable, Pixel,
};
use num_traits::AsPrimitive;
use std::convert::TryFrom;

#[derive(Debug, Clone)]
pub struct ScrollableCanvas {
    width: u32,
    height: u32,
    canvas: BitVec,
}

impl ScrollableCanvas {
    pub fn new(width: u32, height: u32) -> Self {
        let mut canvas = BitVec::new();
        let pixels = width * height;
        canvas.resize(pixels as usize, false);
        Self {
            width,
            height,
            canvas,
        }
    }
}

impl OriginDimensions for ScrollableCanvas {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

impl DrawTarget for ScrollableCanvas {
    type Color = BinaryColor;
    type Error = anyhow::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            let (x, y) = (coord.x, coord.y);
            if x >= 0 && x < (self.width as i32) && y >= 0 && y < (self.height as i32) {
                let index = x + y * self.width as i32;
                self.canvas.set(index.as_(), color.is_on());
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.canvas.fill(color.is_on());
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScrollableBuilder {
    spacing: Option<u32>,
    position: Option<Point>,
    projection: Option<Size>,
    font: Option<&'static MonoFont<'static>>,
    text: String,
}

#[derive(Debug, Clone)]
pub struct StatefulScrollable {
    builder: ScrollableBuilder,
    pub text: Scrollable,
}

impl TryFrom<ScrollableBuilder> for StatefulScrollable {
    type Error = anyhow::Error;

    fn try_from(value: ScrollableBuilder) -> Result<Self, Self::Error> {
        let text = value.build()?;
        Ok(StatefulScrollable {
            builder: value,
            text,
        })
    }
}

impl StatefulScrollable {
    /// Re-renders the scrollable text if the text changed. Returns `Ok(true)`
    /// if the text was updated, `Ok(false)` if the text was not updated or
    /// `Err(_)` if an error occurred during re-rendering.
    ///
    /// # Arguments
    ///
    /// * `text`: the new text
    ///
    /// returns: Result<bool, Error>
    ///
    /// # Examples
    ///
    /// ```
    /// let mut text: StatefulScrollableText = ScrollableTextBuilder::new()
    ///                                                     .with_text("foo")
    ///                                                     .try_into()?;
    /// // Text now displays "foo"
    /// text.update("bar")?;
    /// // Text now displays "bar"
    /// ```
    pub fn update(&mut self, text: &str) -> Result<bool> {
        if self.builder.text != text {
            // TODO: Find a better way?
            let new_builder = self.builder.clone().with_text(text);
            let text = new_builder.build()?;
            self.builder = new_builder;
            self.text = text;
            return Ok(true);
        }
        Ok(false)
    }
}

impl ScrollableBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    pub fn with_custom_spacing(mut self, spacing: u32) -> Self {
        self.spacing = Some(spacing);
        self
    }

    pub fn with_position(mut self, position: Point) -> Self {
        self.position = Some(position);
        self
    }

    pub fn with_projection(mut self, projection: Size) -> Self {
        self.projection = Some(projection);
        self
    }

    #[allow(dead_code)]
    pub fn with_custom_font(mut self, font: &'static MonoFont<'static>) -> Self {
        self.font = Some(font);
        self
    }

    fn calculate_spacing(&self) -> u32 {
        self.spacing.unwrap_or(5)
    }

    fn calculate_size(&self, renderer: &MonoTextStyle<BinaryColor>) -> Size {
        let metrics = renderer.measure_string(&self.text, Point::new(0, 0), Baseline::Top);
        metrics.bounding_box.size + Size::new(self.calculate_spacing(), 0)
    }

    fn default_font() -> &'static MonoFont<'static> {
        &FONT_6X10
    }

    pub fn build(&self) -> Result<Scrollable> {
        let renderer = MonoTextStyleBuilder::new()
            .font(self.font.unwrap_or_else(Self::default_font))
            .text_color(BinaryColor::On)
            .build();
        let size = self.calculate_size(&renderer);
        let mut canvas = ScrollableCanvas::new(size.width, size.height);

        Text::with_baseline(&self.text, Point::new(0, 0), renderer, Baseline::Top)
            .draw(&mut canvas)?;

        Ok(Scrollable {
            canvas,
            projection: self.projection.unwrap_or(size),
            position: self.position.unwrap_or_default(),
            spacing: self.calculate_spacing(),
            scroll: 0,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Scrollable {
    pub canvas: ScrollableCanvas,
    pub projection: Size,
    pub position: Point,
    pub spacing: u32,
    pub scroll: u32,
}

impl Drawable for Scrollable {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, <D as DrawTarget>::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        self.at_tick(target, self.scroll)?;
        Ok::<Self::Output, <D as DrawTarget>::Error>(())
    }
}

impl Scrollable {
    pub fn at_tick<D>(&self, target: &mut D, tick: u32) -> Result<(), <D as DrawTarget>::Error>
    where
        D: DrawTarget<Color = <Scrollable as Drawable>::Color>,
    {
        // TODO: There's probably some really cool bitwise hacks to do here...
        let scroll = tick % self.canvas.width;
        let pixels = self.projection.height * self.projection.width;
        // We know exactly how many pixels we can push so we can pre-allocate exactly.
        let mut pixels = Vec::with_capacity(pixels as usize);

        for n in 0..self.projection.height {
            let min = scroll + n * self.canvas.width;
            let max = (min + self.projection.width).min((n + 1) * self.canvas.width);
            // First draw until we would overflow in the current line
            for i in min..max {
                let coord = Point::new((i - min) as i32, n as i32);
                let color = self.canvas.canvas[i as usize];
                pixels.push(Pixel(self.position + coord, BinaryColor::from(color)));
            }

            // We've reached the end and need to render something from the start
            // Don't do this though if our projection space is larger than our canvas
            // We'd be rendering stuff twice otherwise
            if scroll + self.projection.width >= self.canvas.width
                && self.projection.width < self.canvas.width
            {
                let min = n * self.canvas.width;
                let overflow = scroll + self.projection.width - self.canvas.width;
                let max = min + overflow;

                for i in min..max {
                    let coord = Point::new(
                        (i - min + (self.projection.width - overflow)) as i32,
                        n as i32,
                    );
                    if (i as usize) < self.canvas.canvas.len() {
                        let color = self.canvas.canvas[i as usize];
                        pixels.push(Pixel(self.position + coord, BinaryColor::from(color)));
                    }
                }
            }
        }

        target.draw_iter(pixels.into_iter())?;
        Ok(())
    }

    pub fn scroll(&mut self) {
        self.scroll += 1;
    }
}
