pub fn gltf_to_glam_tranform(gltf_tranform: &gltf::scene::Transform) -> glam::Mat4 {
    glam::Mat4::from_cols_array_2d(&gltf_tranform.clone().matrix())
}

pub fn convert_image_to_bgra8(
    image: &gltf::image::Data,
) -> image::ImageBuffer<image::Bgra<u8>, Vec<u8>> {
    use image::DynamicImage;
    let bgra8;

    match image.format {
        gltf::image::Format::R8G8B8 => {
            let img =
                image::RgbImage::from_vec(image.width, image.height, image.pixels.clone()).unwrap();
            bgra8 = DynamicImage::ImageRgb8(img).into_bgra8();
        }
        gltf::image::Format::R8G8B8A8 => {
            let img = image::ImageBuffer::from_vec(image.width, image.height, image.pixels.clone())
                .unwrap();
            bgra8 = DynamicImage::ImageRgba8(img).into_bgra8();
        }
        gltf::image::Format::B8G8R8 => {
            let img = image::ImageBuffer::from_vec(image.width, image.height, image.pixels.clone())
                .unwrap();
            bgra8 = DynamicImage::ImageBgr8(img).into_bgra8();
        }
        gltf::image::Format::B8G8R8A8 => {
            let img = image::ImageBuffer::from_vec(image.width, image.height, image.pixels.clone())
                .unwrap();
            bgra8 = DynamicImage::ImageBgra8(img).into_bgra8();
        }
        _ => {
            unimplemented!()
        }
    };
    bgra8
}
