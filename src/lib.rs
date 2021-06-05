use std::path::Path;

use image::buffer::ConvertBuffer;

pub struct Scene {
    buffers: Vec<maligog::Buffer>,
    images: Vec<maligog::Image>,
}

impl Scene {
    pub fn from_file<I: AsRef<Path>>(
        name: Option<&str>,
        device: &maligog::Device,
        path: I,
    ) -> Self {
        let (doc, gltf_buffers, gltf_images) = gltf::import(path).unwrap();
        let buffers = gltf_buffers
            .iter()
            .map(|data| {
                device.create_buffer_init(
                    Some("gltf buffer"),
                    data.as_ref(),
                    maligog::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                        | maligog::BufferUsageFlags::STORAGE_BUFFER,
                    maligog::MemoryLocation::GpuOnly,
                )
            })
            .collect::<Vec<_>>();

        use image::DynamicImage;
        let images = gltf_images
            .iter()
            .map(|image| {
                let mut format = maligog::Format::B8G8R8A8_UNORM;
                let bgra8;
                match image.format {
                    gltf::image::Format::R8G8B8 => {
                        let img = image::RgbImage::from_vec(
                            image.width,
                            image.height,
                            image.pixels.clone(),
                        )
                        .unwrap();
                        bgra8 = DynamicImage::ImageRgb8(img).into_bgra8();
                    }
                    gltf::image::Format::R8G8B8A8 => {
                        let img = image::ImageBuffer::from_vec(
                            image.width,
                            image.height,
                            image.pixels.clone(),
                        )
                        .unwrap();
                        bgra8 = DynamicImage::ImageRgba8(img).into_bgra8();
                    }
                    gltf::image::Format::B8G8R8 => {
                        let img = image::ImageBuffer::from_vec(
                            image.width,
                            image.height,
                            image.pixels.clone(),
                        )
                        .unwrap();
                        bgra8 = DynamicImage::ImageBgr8(img).into_bgra8();
                    }
                    gltf::image::Format::B8G8R8A8 => {
                        let img = image::ImageBuffer::from_vec(
                            image.width,
                            image.height,
                            image.pixels.clone(),
                        )
                        .unwrap();
                        bgra8 = DynamicImage::ImageBgra8(img).into_bgra8();
                    }
                    _ => {
                        unimplemented!()
                    }
                };

                device.create_image_init(
                    Some("gltf texture"),
                    format,
                    image.width,
                    image.height,
                    maligog::ImageUsageFlags::SAMPLED,
                    maligog::MemoryLocation::GpuOnly,
                    &bgra8.as_raw(),
                )
            })
            .collect::<Vec<_>>();

        Self { buffers, images }
    }
}

#[test]
fn test_general() {
    dotenv::dotenv().ok();
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .try_init()
        .ok();
    let entry = maligog::Entry::new().unwrap();
    let mut required_extensions = maligog::Surface::required_extensions();
    required_extensions.push(maligog::name::instance::Extension::ExtDebugUtils);
    let instance = entry.create_instance(&[], &&required_extensions);
    let pdevice = instance
        .enumerate_physical_device()
        .first()
        .unwrap()
        .to_owned();
    let device = pdevice.create_device();
    let gltf_test_cases = vec![
        "2.0/Box/glTF/Box.gltf",
        "2.0/BoxInterleaved/glTF/BoxInterleaved.gltf",
        "2.0/Duck/glTF/Duck.gltf",
        "2.0/BoomBox/glTF/BoomBox.gltf",
        "2.0/Sponza/glTF/Sponza.gltf",
        "2.0/GearboxAssy/glTF/GearboxAssy.gltf",
        "2.0/AntiqueCamera/glTF/AntiqueCamera.gltf",
        "2.0/DamagedHelmet/glTF/DamagedHelmet.gltf",
        "2.0/SciFiHelmet/glTF/SciFiHelmet.gltf",
        "2.0/Suzanne/glTF/Suzanne.gltf",
        "2.0/WaterBottle/glTF/WaterBottle.gltf",
        "2.0/2CylinderEngine/glTF/2CylinderEngine.gltf",
        "2.0/Buggy/glTF/Buggy.gltf",
    ];
    for case in gltf_test_cases {
        log::debug!("loading {}", case);
        let gltf_path =
            std::path::PathBuf::from(std::env::var("GLTF_SAMPLE_PATH").unwrap()).join(case);
        let scene = Scene::from_file(Some("test scene"), &device, gltf_path);
    }
}
