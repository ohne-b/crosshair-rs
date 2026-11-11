use anyhow::Result;
use crosshair_core::Text;
use tiny_skia::Path;

pub(crate) struct TextRenderer {
    #[cfg(windows)]
    native: Option<native::Fonts>,
}

impl TextRenderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            #[cfg(windows)]
            native: None,
        })
    }

    #[cfg(windows)]
    fn fonts(&mut self) -> Result<&native::Fonts> {
        if self.native.is_none() {
            self.native = Some(native::Fonts::new()?);
        }
        Ok(self.native.as_ref().expect("font service initialized"))
    }

    pub fn paths(&mut self, text: &Text) -> Result<Vec<Path>> {
        #[cfg(windows)]
        {
            self.fonts()?.paths(text)
        }
        #[cfg(not(windows))]
        {
            let _ = text;
            anyhow::bail!("Text rendering requires Windows DirectWrite")
        }
    }

    pub fn font_families(&mut self) -> Result<Vec<String>> {
        #[cfg(windows)]
        {
            self.fonts()?.families()
        }
        #[cfg(not(windows))]
        {
            anyhow::bail!("System fonts require Windows DirectWrite")
        }
    }
}

#[cfg(windows)]
mod native {
    use std::{
        ffi::c_void,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use anyhow::{Context, Result, ensure};
    use crosshair_core::{Text, TextLayout};
    use tiny_skia::{Path, PathBuilder, Transform};
    use windows::{
        Win32::{
            Foundation::{E_FAIL, E_NOTIMPL, E_POINTER},
            Graphics::{
                Direct2D::Common::{
                    D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN, D2D1_FIGURE_END,
                    D2D1_FIGURE_END_CLOSED, D2D1_FILL_MODE, D2D1_PATH_SEGMENT,
                    ID2D1SimplifiedGeometrySink, ID2D1SimplifiedGeometrySink_Impl,
                },
                DirectWrite::*,
            },
        },
        core::{BOOL, IUnknown, Interface, PCWSTR, Ref, implement, w},
    };
    use windows_numerics::Vector2 as D2D_POINT_2F;

    const MANROPE: &[u8] = include_bytes!("../../../assets/fonts/Manrope.ttf");

    pub struct Fonts {
        factory: IDWriteFactory5,
        collection: IDWriteFontCollection,
        loader: IDWriteInMemoryFontFileLoader,
    }

    impl Fonts {
        pub fn new() -> Result<Self> {
            unsafe {
                let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
                let loader = factory.CreateInMemoryFontFileLoader()?;
                factory.RegisterFontFileLoader(&loader)?;
                let result = (|| -> Result<IDWriteFontCollection> {
                    // Static embedded bytes outlive every collection and renderer.
                    let font = loader.CreateInMemoryFontFileReference(
                        &factory,
                        MANROPE.as_ptr().cast(),
                        MANROPE.len() as u32,
                        None,
                    )?;
                    let builder = factory.CreateFontSetBuilder()?;
                    builder.AddFontFile(&font)?;
                    let base: IDWriteFactory3 = factory.cast()?;
                    builder.AddFontSet(&base.GetSystemFontSet()?)?;
                    Ok(base
                        .CreateFontCollectionFromFontSet(&builder.CreateFontSet()?)?
                        .cast()?)
                })();
                match result {
                    Ok(collection) => Ok(Self {
                        factory,
                        collection,
                        loader,
                    }),
                    Err(error) => {
                        let _ = factory.UnregisterFontFileLoader(&loader);
                        Err(error)
                    }
                }
            }
        }

        pub fn families(&self) -> Result<Vec<String>> {
            let mut result = Vec::new();
            unsafe {
                for index in 0..self.collection.GetFontFamilyCount() {
                    let strings = self.collection.GetFontFamily(index)?.GetFamilyNames()?;
                    let mut locale = 0;
                    let mut exists = BOOL(0);
                    strings.FindLocaleName(w!("en-us"), &mut locale, &mut exists)?;
                    if !exists.as_bool() {
                        locale = 0;
                    }
                    let length = strings.GetStringLength(locale)?;
                    let mut name = vec![0u16; length as usize + 1];
                    strings.GetString(locale, &mut name)?;
                    result.push(String::from_utf16_lossy(&name[..length as usize]));
                }
            }
            result.sort_unstable();
            result.dedup();
            Ok(result)
        }

        fn layout(&self, value: &Text, content: &str) -> Result<IDWriteTextLayout1> {
            let family: Vec<u16> = value.font_family.encode_utf16().chain(Some(0)).collect();
            let content: Vec<u16> = content.encode_utf16().collect();
            unsafe {
                let mut index = 0;
                let mut exists = BOOL(0);
                self.collection
                    .FindFamilyName(PCWSTR(family.as_ptr()), &mut index, &mut exists)?;
                ensure!(
                    exists.as_bool(),
                    "Font '{}' is not installed; choose a replacement",
                    value.font_family
                );
                let format = self.factory.CreateTextFormat(
                    PCWSTR(family.as_ptr()),
                    &self.collection,
                    DWRITE_FONT_WEIGHT(i32::from(value.font_weight)),
                    if value.italic {
                        DWRITE_FONT_STYLE_ITALIC
                    } else {
                        DWRITE_FONT_STYLE_NORMAL
                    },
                    DWRITE_FONT_STRETCH_NORMAL,
                    value.size,
                    w!("en-us"),
                )?;
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                let layout: IDWriteTextLayout1 = self
                    .factory
                    .CreateTextLayout(&content, &format, 65_536.0, 65_536.0)?
                    .cast()?;
                layout.SetCharacterSpacing(
                    0.0,
                    value.spacing,
                    0.0,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: content.len() as u32,
                    },
                )?;
                Ok(layout)
            }
        }

        pub fn paths(&self, value: &Text) -> Result<Vec<Path>> {
            if value.content.is_empty() {
                return Ok(Vec::new());
            }
            ensure!(
                value.content.chars().count() <= 1024,
                "Text exceeds 1024 characters"
            );
            let mut content = value.content.clone();
            if matches!(value.layout, TextLayout::Circle { .. }) {
                ensure!(
                    !content.contains(['\n', '\r']),
                    "Circular text must use one line"
                );
            }
            if let TextLayout::Circle {
                radius,
                repeat: true,
                ..
            } = value.layout
            {
                ensure!(
                    !content.contains(['\n', '\r']),
                    "Circular text must use one line"
                );
                let layout = self.layout(value, &content)?;
                let mut metrics = DWRITE_TEXT_METRICS::default();
                unsafe {
                    layout.GetMetrics(&mut metrics)?;
                }
                if metrics.widthIncludingTrailingWhitespace > 0.0 {
                    let maximum = (1024 / content.chars().count().max(1)).max(1);
                    let count = ((std::f32::consts::TAU * radius
                        / metrics.widthIncludingTrailingWhitespace)
                        .floor() as usize)
                        .clamp(1, maximum);
                    content = content.repeat(count);
                }
            }
            let layout = self.layout(value, &content)?;
            let glyphs = Arc::new(Mutex::new(Vec::new()));
            let renderer: IDWriteTextRenderer = GlyphCollector {
                glyphs: glyphs.clone(),
                points: Arc::new(AtomicUsize::new(0)),
            }
            .into();
            unsafe {
                layout.Draw(None,&renderer,0.0,0.0).context("Text could not be converted to outlines; color fonts require a monochrome replacement")?;
            }
            let glyphs = glyphs.lock().unwrap_or_else(|error| error.into_inner());
            let mut paths = Vec::new();
            for glyph in glyphs.iter() {
                let Some(path) = glyph.path.as_ref() else {
                    continue;
                };
                let transform = match value.layout {
                    TextLayout::Straight => Transform::from_translate(glyph.x, glyph.y),
                    TextLayout::Circle {
                        radius,
                        start_angle,
                        ..
                    } => {
                        ensure!(radius > 0.0, "Circular text radius must be positive");
                        let center = glyph.x + glyph.direction * glyph.advance / 2.0;
                        let angle = start_angle.to_radians() + center / radius;
                        let rotation = angle + std::f32::consts::FRAC_PI_2;
                        Transform::from_translate(-glyph.direction * glyph.advance / 2.0, 0.0)
                            .post_concat(Transform::from_row(
                                rotation.cos(),
                                rotation.sin(),
                                -rotation.sin(),
                                rotation.cos(),
                                radius * angle.cos(),
                                radius * angle.sin(),
                            ))
                    }
                };
                paths.push(
                    path.clone()
                        .transform(transform)
                        .context("Text geometry exceeds numeric limits")?,
                );
            }
            if matches!(value.layout, TextLayout::Straight) && !paths.is_empty() {
                let mut bounds = paths[0].bounds();
                for path in &paths[1..] {
                    bounds = bounds
                        .join(&path.bounds())
                        .context("Text bounds are invalid")?;
                }
                let translation = Transform::from_translate(
                    -(bounds.left() + bounds.right()) / 2.0,
                    -(bounds.top() + bounds.bottom()) / 2.0,
                );
                paths = paths
                    .into_iter()
                    .map(|path| {
                        path.transform(translation)
                            .context("Text bounds are invalid")
                    })
                    .collect::<Result<_>>()?;
            }
            Ok(paths)
        }
    }

    impl Drop for Fonts {
        fn drop(&mut self) {
            unsafe {
                let _ = self.factory.UnregisterFontFileLoader(&self.loader);
            }
        }
    }

    struct Glyph {
        path: Option<Path>,
        x: f32,
        y: f32,
        advance: f32,
        direction: f32,
    }

    #[implement(IDWriteTextRenderer)]
    struct GlyphCollector {
        glyphs: Arc<Mutex<Vec<Glyph>>>,
        points: Arc<AtomicUsize>,
    }

    impl IDWritePixelSnapping_Impl for GlyphCollector_Impl {
        fn IsPixelSnappingDisabled(&self, _: *const c_void) -> windows::core::Result<BOOL> {
            Ok(BOOL(1))
        }
        fn GetCurrentTransform(
            &self,
            _: *const c_void,
            transform: *mut DWRITE_MATRIX,
        ) -> windows::core::Result<()> {
            if transform.is_null() {
                return Err(E_POINTER.into());
            }
            unsafe {
                transform.write(DWRITE_MATRIX {
                    m11: 1.0,
                    m22: 1.0,
                    ..Default::default()
                });
            }
            Ok(())
        }
        fn GetPixelsPerDip(&self, _: *const c_void) -> windows::core::Result<f32> {
            Ok(1.0)
        }
    }

    impl IDWriteTextRenderer_Impl for GlyphCollector_Impl {
        fn DrawGlyphRun(
            &self,
            _: *const c_void,
            x: f32,
            y: f32,
            _: DWRITE_MEASURING_MODE,
            run: *const DWRITE_GLYPH_RUN,
            _: *const DWRITE_GLYPH_RUN_DESCRIPTION,
            _: Ref<IUnknown>,
        ) -> windows::core::Result<()> {
            if run.is_null() {
                return Err(E_POINTER.into());
            }
            let run = unsafe { &*run };
            if run.glyphCount > 4096 || run.glyphIndices.is_null() || run.glyphAdvances.is_null() {
                return Err(E_FAIL.into());
            }
            let face = run
                .fontFace
                .as_ref()
                .ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            if let Ok(face) = face.cast::<IDWriteFontFace2>()
                && unsafe { face.IsColorFont() }.as_bool()
            {
                return Err(E_NOTIMPL.into());
            }
            let direction = if run.bidiLevel & 1 == 0 { 1.0 } else { -1.0 };
            let mut cursor = x;
            for index in 0..run.glyphCount as usize {
                let builder = Arc::new(Mutex::new(PathBuilder::new()));
                let sink: ID2D1SimplifiedGeometrySink = GeometrySink {
                    builder: builder.clone(),
                    points: self.points.clone(),
                }
                .into();
                let advance = unsafe { *run.glyphAdvances.add(index) };
                unsafe {
                    face.GetGlyphRunOutline(
                        run.fontEmSize,
                        run.glyphIndices.add(index),
                        Some(&advance),
                        if run.glyphOffsets.is_null() {
                            None
                        } else {
                            Some(run.glyphOffsets.add(index))
                        },
                        1,
                        run.isSideways.as_bool(),
                        direction < 0.0,
                        &sink,
                    )?;
                    sink.Close()?;
                }
                let path =
                    std::mem::take(&mut *builder.lock().unwrap_or_else(|error| error.into_inner()))
                        .finish();
                let mut glyphs = self
                    .glyphs
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if glyphs.len() >= 4096 {
                    return Err(E_FAIL.into());
                }
                glyphs.push(Glyph {
                    path,
                    x: cursor,
                    y,
                    advance,
                    direction,
                });
                cursor += advance * direction;
            }
            Ok(())
        }
        fn DrawUnderline(
            &self,
            _: *const c_void,
            _: f32,
            _: f32,
            _: *const DWRITE_UNDERLINE,
            _: Ref<IUnknown>,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn DrawStrikethrough(
            &self,
            _: *const c_void,
            _: f32,
            _: f32,
            _: *const DWRITE_STRIKETHROUGH,
            _: Ref<IUnknown>,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn DrawInlineObject(
            &self,
            _: *const c_void,
            _: f32,
            _: f32,
            _: Ref<IDWriteInlineObject>,
            _: BOOL,
            _: BOOL,
            _: Ref<IUnknown>,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }
    }

    #[implement(ID2D1SimplifiedGeometrySink)]
    struct GeometrySink {
        builder: Arc<Mutex<PathBuilder>>,
        points: Arc<AtomicUsize>,
    }

    impl ID2D1SimplifiedGeometrySink_Impl for GeometrySink_Impl {
        fn SetFillMode(&self, _: D2D1_FILL_MODE) {}
        fn SetSegmentFlags(&self, _: D2D1_PATH_SEGMENT) {}
        fn BeginFigure(&self, start: &D2D_POINT_2F, _: D2D1_FIGURE_BEGIN) {
            if self.points.fetch_add(1, Ordering::Relaxed) >= 1_000_000 {
                return;
            }
            self.builder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .move_to(start.X, start.Y);
        }
        fn AddLines(&self, points: *const D2D_POINT_2F, count: u32) {
            if points.is_null()
                || self
                    .points
                    .fetch_add(count as usize, Ordering::Relaxed)
                    .saturating_add(count as usize)
                    > 1_000_000
            {
                return;
            }
            let mut builder = self
                .builder
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for point in unsafe { std::slice::from_raw_parts(points, count as usize) } {
                builder.line_to(point.X, point.Y);
            }
        }
        fn AddBeziers(&self, beziers: *const D2D1_BEZIER_SEGMENT, count: u32) {
            if beziers.is_null()
                || self
                    .points
                    .fetch_add(count as usize * 3, Ordering::Relaxed)
                    .saturating_add(count as usize * 3)
                    > 1_000_000
            {
                return;
            }
            let mut builder = self
                .builder
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for curve in unsafe { std::slice::from_raw_parts(beziers, count as usize) } {
                builder.cubic_to(
                    curve.point1.X,
                    curve.point1.Y,
                    curve.point2.X,
                    curve.point2.Y,
                    curve.point3.X,
                    curve.point3.Y,
                );
            }
        }
        fn EndFigure(&self, end: D2D1_FIGURE_END) {
            if end == D2D1_FIGURE_END_CLOSED {
                self.builder
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .close();
            }
        }
        fn Close(&self) -> windows::core::Result<()> {
            if self.points.load(Ordering::Relaxed) > 1_000_000 {
                Err(E_FAIL.into())
            } else {
                Ok(())
            }
        }
    }
}
