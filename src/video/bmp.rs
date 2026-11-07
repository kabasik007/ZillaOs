//! Strict BMP decoder for a boot-packaged wallpaper, no allocation or I/O.
//! Accept only Windows BITMAPINFOHEADER (40 bytes), BI_RGB, 24/32-bpp,
//! and bounded top-down/bottom-up images. Never trust file dimensions,
//! offsets or row-stride arithmetic without checking them first.

pub const MAX_WIDTH: usize = 1024;
pub const MAX_HEIGHT: usize = 768;
pub const MAX_PIXELS: usize = MAX_WIDTH * MAX_HEIGHT;
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmpError {
    FileTooSmall,
    FileTooLarge,
    InvalidSignature,
    UnsupportedHeader,
    InvalidDimensions,
    UnsupportedFormat,
    InvalidPixelData,
    OutputTooSmall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: usize,
    pub height: usize,
    row_stride: usize,
    offset: usize,
    bytes_per_pixel: usize,
    bottom_up: bool,
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let data = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([data[0], data[1]]))
}
fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let data = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

impl ImageInfo {
    pub fn inspect(bytes: &[u8]) -> Result<Self, BmpError> {
        if bytes.len() < 54 {
            return Err(BmpError::FileTooSmall);
        }
        if bytes.len() > MAX_FILE_BYTES {
            return Err(BmpError::FileTooLarge);
        }
        if bytes.get(..2) != Some(b"BM") {
            return Err(BmpError::InvalidSignature);
        }
        let declared = u32_at(bytes, 2).ok_or(BmpError::FileTooSmall)? as usize;
        if declared != bytes.len() || u32_at(bytes, 14) != Some(40) {
            return Err(BmpError::UnsupportedHeader);
        }
        if u16_at(bytes, 26) != Some(1) || u32_at(bytes, 30) != Some(0) {
            return Err(BmpError::UnsupportedFormat);
        }
        let bpp = u16_at(bytes, 28).ok_or(BmpError::FileTooSmall)?;
        let bytes_per_pixel = match bpp {
            24 => 3,
            32 => 4,
            _ => return Err(BmpError::UnsupportedFormat),
        };
        let signed_width = i32::from_le_bytes(
            bytes[18..22]
                .try_into()
                .map_err(|_| BmpError::FileTooSmall)?,
        );
        let signed_height = i32::from_le_bytes(
            bytes[22..26]
                .try_into()
                .map_err(|_| BmpError::FileTooSmall)?,
        );
        if signed_width <= 0 || signed_height == 0 || signed_height == i32::MIN {
            return Err(BmpError::InvalidDimensions);
        }
        let width = usize::try_from(signed_width).map_err(|_| BmpError::InvalidDimensions)?;
        let height = usize::try_from(signed_height.unsigned_abs())
            .map_err(|_| BmpError::InvalidDimensions)?;
        if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(BmpError::InvalidDimensions);
        }
        let width_bytes = width
            .checked_mul(bytes_per_pixel)
            .ok_or(BmpError::InvalidPixelData)?;
        let row_stride = width_bytes
            .checked_add(3)
            .ok_or(BmpError::InvalidPixelData)?
            & !3;
        let offset = u32_at(bytes, 10).ok_or(BmpError::FileTooSmall)? as usize;
        let pixel_end = row_stride
            .checked_mul(height)
            .and_then(|n| offset.checked_add(n))
            .ok_or(BmpError::InvalidPixelData)?;
        if offset < 54 || pixel_end > bytes.len() {
            return Err(BmpError::InvalidPixelData);
        }
        // In BI_RGB, image size may legitimately be 0. Otherwise it must
        // agree with the required padded stride to avoid ambiguous payloads.
        let declared_image = u32_at(bytes, 34).ok_or(BmpError::FileTooSmall)? as usize;
        if declared_image != 0 && declared_image != row_stride * height {
            return Err(BmpError::InvalidPixelData);
        }
        Ok(Self {
            width,
            height,
            row_stride,
            offset,
            bytes_per_pixel,
            bottom_up: signed_height > 0,
        })
    }

    pub fn pixel(self, bytes: &[u8], x: usize, y: usize) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let source_y = if self.bottom_up {
            self.height - 1 - y
        } else {
            y
        };
        let index = self
            .offset
            .checked_add(source_y.checked_mul(self.row_stride)?)?
            .checked_add(x.checked_mul(self.bytes_per_pixel)?)?;
        let bgr = bytes.get(index..index.checked_add(3)?)?;
        Some((u32::from(bgr[2]) << 16) | (u32::from(bgr[1]) << 8) | u32::from(bgr[0]))
    }

    /// Decode only the compact width*height pixel prefix into caller-owned RAM.
    /// Caller can keep that RAM after Limine boot data becomes reclaimable.
    pub fn decode_into(self, bytes: &[u8], out: &mut [u32]) -> Result<(), BmpError> {
        let count = self
            .width
            .checked_mul(self.height)
            .ok_or(BmpError::InvalidDimensions)?;
        if out.len() < count {
            return Err(BmpError::OutputTooSmall);
        }
        for y in 0..self.height {
            let base = y * self.width;
            for x in 0..self.width {
                out[base + x] = self.pixel(bytes, x, y).ok_or(BmpError::InvalidPixelData)?;
            }
        }
        Ok(())
    }
}

/// Nearest-neighbour pixel mapping, using u64 to avoid multiplication overflow
/// even if the destination boot resolution is 8192x8192.
pub fn scaled_coord(point: usize, source: usize, dest: usize) -> Option<usize> {
    if dest == 0 || source == 0 || point >= dest {
        return None;
    }
    let coord =
        (u64::try_from(point).ok()? * u64::try_from(source).ok()?) / u64::try_from(dest).ok()?;
    usize::try_from(coord).ok().map(|v| v.min(source - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp(width: i32, height: i32, bpp: u16) -> [u8; 78] {
        // Two rows, 3 pixels each in our test fixtures. 24-bit rows padded
        // to 12 bytes; total header 54 + 2*12 bytes.
        let mut data = [0u8; 78];
        data[0..2].copy_from_slice(b"BM");
        data[2..6].copy_from_slice(&(78u32).to_le_bytes());
        data[10..14].copy_from_slice(&(54u32).to_le_bytes());
        data[14..18].copy_from_slice(&(40u32).to_le_bytes());
        data[18..22].copy_from_slice(&width.to_le_bytes());
        data[22..26].copy_from_slice(&height.to_le_bytes());
        data[26..28].copy_from_slice(&(1u16).to_le_bytes());
        data[28..30].copy_from_slice(&bpp.to_le_bytes());
        data
    }

    #[test]
    fn bottom_up_24_bpp_and_stride_padding() {
        let mut bytes = bmp(3, 2, 24);
        // Bottom row (y=1): blue, green, red.
        bytes[54..63].copy_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255]);
        // Top row (y=0): white, black, yellow.
        bytes[66..75].copy_from_slice(&[255, 255, 255, 0, 0, 0, 0, 255, 255]);
        let header = ImageInfo::inspect(&bytes).unwrap();
        assert_eq!((header.width, header.height), (3, 2));
        assert_eq!(header.pixel(&bytes, 0, 0), Some(0xffffff));
        assert_eq!(header.pixel(&bytes, 1, 0), Some(0));
        assert_eq!(header.pixel(&bytes, 2, 0), Some(0xffff00));
        assert_eq!(header.pixel(&bytes, 0, 1), Some(0x0000ff));
        let mut output = [0u32; 6];
        header.decode_into(&bytes, &mut output).unwrap();
        assert_eq!(
            output,
            [0xffffff, 0, 0xffff00, 0x0000ff, 0x00ff00, 0xff0000]
        );
    }

    #[test]
    fn negative_height_top_down_and_32_bpp() {
        let mut bytes = bmp(3, -2, 32);
        // 32bit stride * 2 requires 24 bytes exactly.
        bytes[54..58].copy_from_slice(&[0x12, 0x34, 0x56, 0xff]);
        let header = ImageInfo::inspect(&bytes).unwrap();
        assert_eq!(header.pixel(&bytes, 0, 0), Some(0x563412));
        assert_eq!(header.pixel(&bytes, 0, 1), Some(0));
    }

    #[test]
    fn rejects_corrupt_headers_sizes_and_unsupported_compression() {
        let valid = bmp(3, 2, 24);
        for v in [
            {
                let mut a = valid;
                a[0] = 0;
                a
            },
            {
                let mut a = valid;
                a[14] = 12;
                a
            },
            {
                let mut a = valid;
                a[26] = 0;
                a
            },
            {
                let mut a = valid;
                a[28] = 8;
                a
            },
            {
                let mut a = valid;
                a[30] = 1;
                a
            },
            {
                let mut a = valid;
                a[10] = 1;
                a
            },
            {
                let mut a = valid;
                a[2] = 0;
                a
            },
            {
                let mut a = valid;
                a[18..22].copy_from_slice(&(1025i32).to_le_bytes());
                a
            },
            {
                let mut a = valid;
                a[22..26].copy_from_slice(&(i32::MIN).to_le_bytes());
                a
            },
        ] {
            assert!(ImageInfo::inspect(&v).is_err());
        }
        assert_eq!(
            ImageInfo::inspect(&valid[..12]),
            Err(BmpError::FileTooSmall)
        );
        let mut short = valid;
        short[10..14].copy_from_slice(&(77u32).to_le_bytes());
        assert_eq!(ImageInfo::inspect(&short), Err(BmpError::InvalidPixelData));
    }

    #[test]
    fn output_bounds_and_scaling_are_checked() {
        let bytes = bmp(3, 2, 24);
        let header = ImageInfo::inspect(&bytes).unwrap();
        assert_eq!(
            header.decode_into(&bytes, &mut [0u32; 5]),
            Err(BmpError::OutputTooSmall)
        );
        assert_eq!(scaled_coord(0, 3, 1920), Some(0));
        assert_eq!(scaled_coord(1919, 3, 1920), Some(2));
        assert_eq!(scaled_coord(1920, 3, 1920), None);
        assert_eq!(scaled_coord(0, 3, 0), None);
    }
}
