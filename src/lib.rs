use std::path::Path;

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

        let images = gltf_images
            .iter()
            .map(|image| {
                let format = match image.format {
                    gltf::image::Format::R8 => maligog::Format::R8_UNORM,
                    gltf::image::Format::R8G8 => maligog::Format::R8G8_UNORM,
                    gltf::image::Format::R8G8B8 => maligog::Format::R8G8B8_UNORM,
                    gltf::image::Format::R8G8B8A8 => maligog::Format::R8G8B8A8_UNORM,
                    gltf::image::Format::B8G8R8 => maligog::Format::B8G8R8_UNORM,
                    gltf::image::Format::B8G8R8A8 => maligog::Format::B8G8R8A8_UNORM,
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
                    &image.pixels,
                )
            })
            .collect::<Vec<_>>();

        Self { buffers, images }
    }
}

#[test]
fn test_general() {
    dotenv::dotenv().ok();
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
    let scene = Scene::from_file(
        Some("test scene"),
        &device,
        std::env::var("GLTF_TEST_FILE").unwrap(),
    );
}
