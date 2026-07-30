use image::DynamicImage;
use ratatui_image::FontSize;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::collections::HashMap;

#[derive(Default)]
pub struct ImageRuntime {
    pub picker: Option<Picker>,
    protocols: HashMap<(String, u16, u16), Option<StatefulProtocol>>,
}

impl ImageRuntime {
    pub fn clear(&mut self) {
        self.protocols.clear();
    }

    pub fn protocol(
        &mut self,
        url: &str,
        bytes: &[u8],
        columns: u16,
        rows: u16,
    ) -> Option<&mut StatefulProtocol> {
        let picker = self.picker.as_mut()?;
        let key = (url.to_string(), columns, rows);
        if !self.protocols.contains_key(&key) {
            let protocol = decode_image(bytes).map(|decoded| {
                let sized = fit_to_cells(decoded, picker.font_size(), columns, rows);
                picker.new_resize_protocol(sized)
            });
            self.protocols.insert(key.clone(), protocol);
        }
        self.protocols.get_mut(&key)?.as_mut()
    }
}

pub fn decode_image(bytes: &[u8]) -> Option<DynamicImage> {
    image::load_from_memory(bytes).ok()
}

pub fn pixel_box(font_size: FontSize, columns: u16, rows: u16) -> (u32, u32) {
    (
        u32::from(columns) * u32::from(font_size.width),
        u32::from(rows) * u32::from(font_size.height),
    )
}

fn fit_to_cells(image: DynamicImage, font_size: FontSize, columns: u16, rows: u16) -> DynamicImage {
    let (width, height) = pixel_box(font_size, columns, rows);
    if width == 0 || height == 0 {
        return image;
    }
    image.thumbnail(width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        let pixels = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 40, 40, 255]));
        let mut cursor = std::io::Cursor::new(Vec::new());
        pixels
            .write_to(&mut cursor, image::ImageFormat::Png)
            .expect("encoding a tiny png succeeds");
        cursor.into_inner()
    }

    #[test]
    fn decode_image_accepts_png_and_rejects_garbage() {
        struct Case {
            name: &'static str,
            bytes: Vec<u8>,
            want_some: bool,
        }
        let cases = [
            Case {
                name: "valid png decodes",
                bytes: png_bytes(),
                want_some: true,
            },
            Case {
                name: "html error page is rejected",
                bytes: b"<html>not found</html>".to_vec(),
                want_some: false,
            },
            Case {
                name: "empty payload is rejected",
                bytes: Vec::new(),
                want_some: false,
            },
        ];
        for case in cases {
            assert_eq!(
                decode_image(&case.bytes).is_some(),
                case.want_some,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn pixel_box_scales_cells_by_font_size() {
        struct Case {
            name: &'static str,
            font_size: FontSize,
            columns: u16,
            rows: u16,
            want: (u32, u32),
        }
        let cases = [
            Case {
                name: "typical terminal font",
                font_size: FontSize {
                    width: 8,
                    height: 16,
                },
                columns: 40,
                rows: 12,
                want: (320, 192),
            },
            Case {
                name: "zero columns collapse",
                font_size: FontSize {
                    width: 8,
                    height: 16,
                },
                columns: 0,
                rows: 12,
                want: (0, 192),
            },
        ];
        for case in cases {
            assert_eq!(
                pixel_box(case.font_size, case.columns, case.rows),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn runtime_without_picker_never_yields_protocols() {
        let mut runtime = ImageRuntime::default();
        assert!(
            runtime
                .protocol("https://img.example/a.png", &png_bytes(), 40, 12)
                .is_none()
        );
    }
}
