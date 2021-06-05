#![feature(inline_const)]
#![feature(const_type_id)]

use std::convert::TryInto;
use std::path::Path;

use image::buffer::ConvertBuffer;

use std::any::{Any, TypeId};

pub struct Scene {
    buffers: Vec<maligog::Buffer>,
    images: Vec<maligog::Image>,
    tlas: maligog::TopAccelerationStructure,
}

fn convert_image_to_bgra8(
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

fn create_device_buffers(
    device: &maligog::Device,
    gltf_buffers: &[gltf::buffer::Data],
) -> Vec<maligog::Buffer> {
    gltf_buffers
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
        .collect::<Vec<_>>()
}

fn create_device_images(
    device: &maligog::Device,
    gltf_images: &[gltf::image::Data],
) -> Vec<maligog::Image> {
    gltf_images
        .iter()
        .map(|image| {
            let mut format = maligog::Format::B8G8R8A8_UNORM;
            let bgra8 = convert_image_to_bgra8(image);
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
        .collect::<Vec<_>>()
}

fn process_node(
    device: &maligog::Device,
    node: &gltf::Node,
    blases: &[maligog::BottomAccelerationStructure],
) -> Vec<maligog::BLASInstance> {
    let mut instances = Vec::new();
    if let Some(mesh) = node.mesh() {
        instances.push(maligog::BLASInstance::new(
            &device,
            &blases.get(mesh.index()).unwrap(),
            &glam::Mat4::from_cols_array_2d(&node.transform().matrix()),
        ));
    }
    instances.extend(
        node.children()
            .map(|n| process_node(&device, &n, blases))
            .flatten()
            .map(|mut i| {
                i.set_transform(
                    &i.transform()
                        .mul_mat4(&glam::Mat4::from_cols_array_2d(&node.transform().matrix())),
                );
                i
            })
            .collect::<Vec<_>>(),
    );
    instances
}

fn create_BLASes(
    device: &maligog::Device,
    gltf_meshes: gltf::iter::Meshes,
    buffers: &[maligog::Buffer],
) -> Vec<maligog::BottomAccelerationStructure> {
    let mut BLASes = Vec::new();
    for mesh in gltf_meshes {
        let geometries: Vec<maligog::TriangleGeometry> = mesh
            .primitives()
            .map(|p| {
                let index_accessor = p.indices().unwrap();
                let (_, vertex_accessor) = p
                    .attributes()
                    .find(|(semantic, _)| semantic.eq(&gltf::Semantic::Positions))
                    .unwrap();
                let index_buffer_view = maligog::IndexBufferView {
                    buffer_view: maligog::BufferView {
                        buffer: buffers[index_accessor.view().unwrap().buffer().index()].clone(),
                        offset: (index_accessor.offset() + index_accessor.view().unwrap().offset())
                            as u64,
                    },
                    index_type: match index_accessor.data_type() {
                        gltf::accessor::DataType::U16 => maligog::IndexType::UINT16,
                        gltf::accessor::DataType::U32 => maligog::IndexType::UINT32,
                        _ => {
                            unimplemented!()
                        }
                    },
                    count: index_accessor.count() as u32,
                };
                let vertex_buffer_view = maligog::VertexBufferView {
                    buffer_view: maligog::BufferView {
                        buffer: buffers[vertex_accessor.view().unwrap().buffer().index()].clone(),
                        offset: (vertex_accessor.offset()
                            + vertex_accessor.view().unwrap().offset())
                            as u64,
                    },
                    format: match vertex_accessor.data_type() {
                        gltf::accessor::DataType::U32 => maligog::Format::R32G32B32_UINT,
                        gltf::accessor::DataType::F32 => maligog::Format::R32G32B32_SFLOAT,
                        _ => {
                            unimplemented!()
                        }
                    },
                    stride: match vertex_accessor.dimensions() {
                        gltf::accessor::Dimensions::Vec3 => std::mem::size_of::<f32>() as u64 * 3,
                        _ => {
                            unimplemented!()
                        }
                    },
                    count: vertex_accessor.count() as u32,
                };

                maligog::TriangleGeometry::new(&index_buffer_view, &vertex_buffer_view, None)
            })
            .collect();

        BLASes.push(device.create_bottom_level_acceleration_structure(mesh.name(), &geometries));
    }
    BLASes
}

impl Scene {
    pub fn from_file<I: AsRef<Path>>(
        name: Option<&str>,
        device: &maligog::Device,
        path: I,
    ) -> Self {
        let (doc, gltf_buffers, gltf_images) = gltf::import(path).unwrap();
        let scene = doc.default_scene().unwrap();

        let buffers = create_device_buffers(device, &gltf_buffers);
        let images = create_device_images(device, &gltf_images);
        let blases = create_BLASes(device, doc.meshes(), &buffers);
        let mut blas_instances = scene
            .nodes()
            .map(|n| {
                let mut instances = process_node(device, &n, &blases);
                for i in instances.as_mut_slice() {
                    i.set_transform(
                        &i.transform()
                            .mul_mat4(&glam::Mat4::from_cols_array_2d(&n.transform().matrix())),
                    );
                }
                instances
            })
            .flatten()
            .collect::<Vec<_>>();
        for instance in blas_instances.as_mut_slice() {
            instance.build();
        }
        let instance_geometry = maligog::InstanceGeometry::new(&device, blas_instances.as_slice());
        let tlas =
            device.create_top_level_acceleration_structure(scene.name(), &[instance_geometry]);

        Self {
            buffers,
            images,
            tlas,
        }
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
