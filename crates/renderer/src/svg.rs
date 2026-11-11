//! SVG parsing is isolated from filesystem resources. The renderer receives only
//! local fragments, bounded embedded bitmaps, and explicitly installed fonts.
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result, bail, ensure};
use crosshair_core::AssetKind;
use resvg::usvg::{self, Node};
use tiny_skia::Transform;

use crate::{AssetData, inspect_asset};

pub(crate) fn parse(bytes: &[u8]) -> Result<usvg::Tree> {
    ensure!(bytes.len() <= 8 * 1024 * 1024, "SVG exceeds 8 MiB");
    let svg = std::str::from_utf8(bytes).context("SVG must use UTF-8")?;
    let document = roxmltree::Document::parse_with_options(
        svg,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 10_000,
            ..Default::default()
        },
    )
    .context("Invalid or overly complex SVG")?;
    let mut path_bytes = 0usize;
    let mut text_bytes = 0usize;
    let mut has_text = false;
    for node in document.descendants().filter(|node| node.is_element()) {
        ensure!(
            node.ancestors().take(66).count() <= 64,
            "SVG nesting exceeds 64 levels"
        );
        match node.tag_name().name() {
            "script" | "foreignObject" | "animate" | "animateTransform" | "animateMotion"
            | "set" => bail!("SVG contains active or animated content"),
            "font" | "font-face" => {
                bail!("Embedded SVG fonts are not supported; use installed fonts or outlines")
            }
            "text" | "tspan" | "textPath" => {
                has_text = true;
                text_bytes += node.text().unwrap_or_default().len();
            }
            _ => {}
        }
        if let Some(octaves) = node.attribute("numOctaves") {
            ensure!(
                octaves.parse::<u8>().is_ok_and(|value| value <= 8),
                "SVG turbulence exceeds eight octaves"
            );
        }
        if let Some(kernel) = node.attribute("kernelMatrix") {
            ensure!(
                kernel
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|part| !part.is_empty())
                    .take(226)
                    .count()
                    <= 225,
                "SVG convolution exceeds 225 kernel values"
            );
        }
        for attribute in node.attributes() {
            let value = attribute.value().trim();
            if attribute.name() == "href" {
                let bitmap = matches!(node.tag_name().name(), "image" | "feImage")
                    && value.starts_with("data:image/");
                ensure!(
                    value.starts_with('#') || bitmap,
                    "SVG external references are not supported"
                );
            }
            ensure!(
                !attribute.name().starts_with("on"),
                "SVG event handlers are not supported"
            );
            if matches!(attribute.name(), "d" | "points") {
                path_bytes = path_bytes.saturating_add(value.len());
            }
            ensure!(
                safe_css(value),
                "SVG external resources and embedded fonts are not supported"
            );
        }
        if node.tag_name().name() == "style" {
            ensure!(
                safe_css(node.text().unwrap_or_default()),
                "SVG external styles and embedded fonts are not supported"
            );
        }
    }
    ensure!(
        path_bytes <= 1024 * 1024 && text_bytes <= 16 * 1024,
        "SVG geometry or text exceeds the complexity limit"
    );
    check_references(&document)?;
    let errors = Arc::new(Mutex::new(Vec::new()));
    let bitmap_pixels = Arc::new(AtomicU64::new(0));
    let image_errors = errors.clone();
    let font_errors = errors.clone();
    let fallback_errors = errors.clone();
    let default_font = usvg::FontResolver::default_font_selector();
    let fallback_font = usvg::FontResolver::default_fallback_selector();
    let options = usvg::Options {
        resources_dir: None,
        font_family: "Manrope".into(),
        fontdb: if has_text {
            fonts().clone()
        } else {
            Arc::new(usvg::fontdb::Database::new())
        },
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, _| {
                let result = (|| -> Result<usvg::ImageKind> {
                    let kind = match mime {
                        "image/png" => AssetKind::Png,
                        "image/jpeg" | "image/jpg" => AssetKind::Jpeg,
                        "image/gif" => AssetKind::Gif,
                        _ => bail!("Embedded SVG images must use PNG, JPEG or GIF"),
                    };
                    let info = inspect_asset(&AssetData {
                        kind: kind.clone(),
                        bytes: Arc::from(data.as_slice()),
                    })?;
                    let pixels = u64::from(info.width) * u64::from(info.height);
                    ensure!(
                        bitmap_pixels
                            .fetch_add(pixels, Ordering::Relaxed)
                            .saturating_add(pixels)
                            <= 4 * 1024 * 1024,
                        "Embedded SVG bitmaps exceed four megapixels in total"
                    );
                    Ok(match kind {
                        AssetKind::Png => usvg::ImageKind::PNG(data),
                        AssetKind::Jpeg => usvg::ImageKind::JPEG(data),
                        AssetKind::Gif => usvg::ImageKind::GIF(data),
                        _ => unreachable!(),
                    })
                })();
                match result {
                    Ok(image) => Some(image),
                    Err(error) => {
                        image_errors
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(error.to_string());
                        None
                    }
                }
            }),
            resolve_string: Box::new(|_, _| None),
        },
        font_resolver: usvg::FontResolver {
            select_font: Box::new(move |font, database| {
                let available = font.families().iter().any(|family| match family {
                    usvg::FontFamily::Named(name) => database.faces().any(|face| {
                        face.families
                            .iter()
                            .any(|(family, _)| family.eq_ignore_ascii_case(name))
                    }),
                    _ => true,
                });
                if !available {
                    font_errors.lock().unwrap_or_else(|e| e.into_inner()).push(format!("SVG font {} is not installed; choose a replacement or convert text to paths", font.families().iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")));
                    return None;
                }
                default_font(font, database)
            }),
            select_fallback: Box::new(move |character, excluded, database| {
                let result = fallback_font(character, excluded, database);
                if result.is_none() {
                    fallback_errors
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(format!(
                            "No installed SVG font supports U+{:04X}",
                            character as u32
                        ));
                }
                result
            }),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &options).context("Unsupported SVG document")?;
    if let Some(error) = errors.lock().unwrap_or_else(|e| e.into_inner()).first() {
        bail!("{error}");
    }
    // Also limits expanded <use> nodes and shaped text, not just the XML source.
    Budget::default().group(tree.root(), Transform::identity(), 0, false)?;
    Ok(tree)
}

fn check_references(document: &roxmltree::Document<'_>) -> Result<()> {
    let ids: std::collections::HashMap<_, _> = document
        .descendants()
        .filter_map(|node| node.attribute("id").map(|id| (id, node)))
        .collect();
    fn visit<'a, 'input>(
        node: roxmltree::Node<'a, 'input>,
        ids: &std::collections::HashMap<&'input str, roxmltree::Node<'a, 'input>>,
        stack: &mut Vec<roxmltree::NodeId>,
        counts: &mut [usize; 3],
    ) -> Result<()> {
        counts[0] += 1;
        counts[1] += node
            .attributes()
            .filter(|attribute| matches!(attribute.name(), "d" | "points"))
            .map(|attribute| attribute.value().len())
            .sum::<usize>();
        if matches!(node.tag_name().name(), "text" | "tspan" | "textPath") {
            counts[2] += node.text().unwrap_or_default().len();
        }
        ensure!(
            counts[0] <= 20_000
                && counts[1] <= 2 * 1024 * 1024
                && counts[2] <= 16 * 1024
                && stack.len() < 64,
            "Expanded SVG references exceed the complexity limit"
        );
        ensure!(
            !stack.contains(&node.id()),
            "SVG contains a circular element reference"
        );
        stack.push(node.id());
        if node.tag_name().name() == "use"
            && let Some(target) = node
                .attributes()
                .find(|attribute| attribute.name() == "href")
                .and_then(|attribute| attribute.value().strip_prefix('#'))
                .and_then(|id| ids.get(id))
        {
            visit(*target, ids, stack, counts)?;
        }
        for child in node.children().filter(|node| node.is_element()) {
            visit(child, ids, stack, counts)?;
        }
        stack.pop();
        Ok(())
    }
    visit(document.root_element(), &ids, &mut Vec::new(), &mut [0; 3])
}

fn safe_css(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("@import") || lower.contains("@font-face") {
        return false;
    }
    lower.split("url(").skip(1).all(|tail| {
        tail.split(')')
            .next()
            .is_some_and(|url| url.trim().trim_matches(['\'', '"']).starts_with('#'))
    })
}

fn fonts() -> &'static Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut database = usvg::fontdb::Database::new();
        database.load_font_data(include_bytes!("../../../assets/fonts/Manrope.ttf").to_vec());
        database.load_system_fonts();
        database.set_sans_serif_family("Manrope");
        database.set_serif_family("Times New Roman");
        database.set_monospace_family("Consolas");
        Arc::new(database)
    })
}

pub(crate) fn check_render_budget(tree: &usvg::Tree, transform: Transform) -> Result<()> {
    Budget::default().group(tree.root(), transform, 0, true)
}

#[derive(Default)]
struct Budget {
    bytes: u64,
    nodes: usize,
    points: usize,
}

impl Budget {
    fn surface(&mut self, width: f32, height: f32, copies: usize) -> Result<()> {
        ensure!(
            width.is_finite() && height.is_finite() && width <= 4096.0 && height <= 4096.0,
            "SVG intermediate surface exceeds 4096 pixels"
        );
        self.bytes = self.bytes.saturating_add(
            (width.max(0.0).ceil() as u64 + 4)
                .saturating_mul(height.max(0.0).ceil() as u64 + 4)
                .saturating_mul(4)
                .saturating_mul(copies as u64),
        );
        // The remaining budget is reserved for embedded image decoding, the
        // document, raster output, and the application's layer buffers.
        ensure!(
            self.bytes <= 32 * 1024 * 1024,
            "SVG effects exceed the working-memory budget; reduce filter regions or output size"
        );
        Ok(())
    }

    fn group(
        &mut self,
        group: &usvg::Group,
        transform: Transform,
        depth: usize,
        surfaces: bool,
    ) -> Result<()> {
        ensure!(depth <= 64, "Expanded SVG nesting exceeds 64 levels");
        let transform = transform.pre_concat(group.transform());
        let (sx, sy) = transform.get_scale();
        if surfaces && group.should_isolate() {
            let rect = group.layer_bounding_box();
            let count = group
                .filters()
                .iter()
                .map(|filter| filter.primitives().len())
                .sum::<usize>();
            let width = (rect.width() * sx)
                .max(rect.width() * transform.sx.abs() + rect.height() * transform.kx.abs());
            let height = (rect.height() * sy)
                .max(rect.width() * transform.ky.abs() + rect.height() * transform.sy.abs());
            self.surface(width, height, count + 6)?;
        }
        for filter in group.filters() {
            ensure!(
                filter.primitives().len() <= 128,
                "SVG filter exceeds 128 operations"
            );
            for primitive in filter.primitives() {
                if let usvg::filter::Kind::Image(image) = primitive.kind() {
                    self.group(
                        image.root(),
                        Transform::from_scale(sx, sy),
                        depth + 1,
                        surfaces,
                    )?;
                }
            }
        }
        let mut mask = group.mask();
        let mut mask_depth = 0;
        while let Some(current) = mask {
            mask_depth += 1;
            ensure!(
                depth + mask_depth <= 64,
                "SVG mask nesting exceeds 64 levels"
            );
            self.group(current.root(), transform, depth + mask_depth, surfaces)?;
            mask = current.mask();
        }
        let mut clip = group.clip_path();
        let mut clip_depth = 0;
        while let Some(current) = clip {
            clip_depth += 1;
            ensure!(
                depth + clip_depth <= 64,
                "SVG clipping nesting exceeds 64 levels"
            );
            self.group(
                current.root(),
                transform.pre_concat(current.transform()),
                depth + clip_depth,
                surfaces,
            )?;
            clip = current.clip_path();
        }
        for node in group.children() {
            self.nodes += 1;
            ensure!(self.nodes <= 20_000, "Expanded SVG exceeds 20,000 nodes");
            match node {
                Node::Group(child) => self.group(child, transform, depth + 1, surfaces)?,
                Node::Text(text) => self.group(text.flattened(), transform, depth + 1, surfaces)?,
                Node::Path(path) => {
                    self.points += path.data().points().len();
                    ensure!(
                        self.points <= 1_000_000,
                        "SVG exceeds one million vector points"
                    );
                    for paint in path
                        .fill()
                        .map(|fill| fill.paint())
                        .into_iter()
                        .chain(path.stroke().map(|stroke| stroke.paint()))
                    {
                        if let usvg::Paint::Pattern(pattern) = paint {
                            let (sx, sy) = transform.pre_concat(pattern.transform()).get_scale();
                            if surfaces {
                                self.surface(
                                    pattern.rect().width() * sx,
                                    pattern.rect().height() * sy,
                                    2,
                                )?;
                            }
                            self.group(
                                pattern.root(),
                                Transform::from_scale(sx, sy),
                                depth + 1,
                                surfaces,
                            )?;
                        }
                    }
                }
                Node::Image(_) => {}
            }
        }
        Ok(())
    }
}
