use ringhopper::definitions::{Bitmap, BitmapData, BitmapDataFlags, BitmapDataFormat, BitmapDataType, BitmapFormat, BitmapGroupSequence, BitmapType, BitmapUsage};
use ringhopper::primitives::primitive::{Data, Reflexive, TagGroup, Vector2DInt};
use ringhopper::tag::bitmap::bits_per_pixel;

#[derive(Copy, Clone)]
pub struct Dimensions {
    pub w: u16,
    pub h: u16
}

pub struct LmPage {
    pub dimensions: Dimensions,
    pub data_format: BitmapDataFormat,
    pub data: Vec<u8>,
}

pub fn get_lm_page(bitmap: &Bitmap, index: u16) -> Result<LmPage, String> {
    let bitmap_data = bitmap.bitmap_data.items.get(index as usize)
        .ok_or(format!("Bitmap does not contain data index {}", index))?;

    let bytes_per_pixel = bits_per_pixel(bitmap_data.format).get() / 8;
    let data_size = bytes_per_pixel * bitmap_data.width as usize * bitmap_data.height as usize;
    let data_offset_start = bitmap_data.pixel_data_offset as usize;
    let data_offset_end = data_offset_start + data_size;
    let data: &[u8] = &bitmap.processed_pixel_data.bytes[data_offset_start..data_offset_end];

    Ok(LmPage {
        dimensions: Dimensions {
            w: bitmap_data.width,
            h: bitmap_data.height,
        },
        data_format: bitmap_data.format,
        data: Vec::from(data),
    })
}

pub fn create_lm_bitmap(pages: &[LmPage]) -> Bitmap {
    let mut pixel_data: Vec<u8> = Vec::new();
    pages.iter().for_each(|page| {
        pixel_data.extend(&page.data);
    });
    Bitmap {
        _type: BitmapType::_2dTextures,
        encoding_format: BitmapFormat::_16Bit,
        usage: BitmapUsage::LightMap,
        processed_pixel_data: Data::new(pixel_data),
        bitmap_group_sequence: Reflexive::new((0..pages.len()).map(|i| {
            BitmapGroupSequence {
                bitmap_count: 1,
                first_bitmap_index: Some(i as u16),
                ..BitmapGroupSequence::default()
            }
        }).collect()),
        bitmap_data: Reflexive::new(pages.iter().enumerate().map(|(lm_bitmap_index, page)| {
            let dimensions = page.dimensions;
            BitmapData {
                signature: TagGroup::Bitmap,
                width: dimensions.w,
                height: dimensions.h,
                depth: 1,
                _type: BitmapDataType::_2dTexture,
                format: page.data_format,
                flags: BitmapDataFlags {
                    power_of_two_dimensions: true,
                    ..BitmapDataFlags::default()
                },
                registration_point: Vector2DInt {
                    x: (dimensions.w / 2) as i16,
                    y: (dimensions.h / 2) as i16,
                },
                mipmap_count: 0,
                pixel_data_offset: (0..lm_bitmap_index)
                    .map(|i| pages[i].data.len() as u32)
                    .sum(),
                ..BitmapData::default()
            }
        }).collect()),
        ..Bitmap::default()
    }
}
