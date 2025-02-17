use image;
use std::io::Cursor;

pub fn ico_to_rgba(
    ico_bytes: &'static [u8],
) -> Result<(Vec<u8>, u32, u32), Box<dyn std::error::Error>> {
    // Read the image from memory
    let img = image::io::Reader::new(Cursor::new(ico_bytes))
        .with_guessed_format()?
        .decode()?;

    // Convert to RGBA8
    let rgba_image = img.to_rgba8();

    // Get dimensions
    let (width, height) = rgba_image.dimensions();

    // Extract raw pixel data
    let rgba_data = rgba_image.into_raw();

    Ok((rgba_data, width, height))
}
