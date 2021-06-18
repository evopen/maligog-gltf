#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused))]

mod util;

pub use gltf;

use std::convert::TryInto;
use std::path::Path;

use image::buffer::ConvertBuffer;

use std::any::{Any, TypeId};

pub struct Scene {
    images: Vec<maligog::Image>,
    tlas: maligog::TopAccelerationStructure,
    samplers: Vec<maligog::Sampler>,
    doc: gltf::Document,
    mesh_data: MeshData,
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
                    | maligog::BufferUsageFlags::STORAGE_BUFFER
                    | maligog::BufferUsageFlags::VERTEX_BUFFER
                    | maligog::BufferUsageFlags::INDEX_BUFFER,
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
            let bgra8 = util::convert_image_to_bgra8(image);
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

fn create_samlers(
    device: &maligog::Device,
    gltf_samplers: gltf::iter::Samplers,
) -> Vec<maligog::Sampler> {
    let mut samplers = vec![];
    for sampler in gltf_samplers {
        let mag_filter = if let Some(mag_filter) = sampler.mag_filter() {
            match mag_filter {
                gltf::texture::MagFilter::Nearest => maligog::Filter::NEAREST,
                gltf::texture::MagFilter::Linear => maligog::Filter::LINEAR,
            }
        } else {
            maligog::Filter::LINEAR
        };

        let min_filter = if let Some(min_filter) = sampler.min_filter() {
            match min_filter {
                gltf::texture::MinFilter::Nearest => maligog::Filter::NEAREST,
                gltf::texture::MinFilter::Linear => maligog::Filter::LINEAR,
                gltf::texture::MinFilter::NearestMipmapNearest => maligog::Filter::NEAREST,
                gltf::texture::MinFilter::LinearMipmapNearest => maligog::Filter::LINEAR,
                gltf::texture::MinFilter::NearestMipmapLinear => maligog::Filter::NEAREST,
                gltf::texture::MinFilter::LinearMipmapLinear => maligog::Filter::LINEAR,
            }
        } else {
            maligog::Filter::LINEAR
        };

        let address_mode_u = match sampler.wrap_s() {
            gltf::texture::WrappingMode::ClampToEdge => maligog::SamplerAddressMode::CLAMP_TO_EDGE,
            gltf::texture::WrappingMode::MirroredRepeat => {
                maligog::SamplerAddressMode::MIRRORED_REPEAT
            }
            gltf::texture::WrappingMode::Repeat => maligog::SamplerAddressMode::REPEAT,
        };
        let address_mode_v = match sampler.wrap_t() {
            gltf::texture::WrappingMode::ClampToEdge => maligog::SamplerAddressMode::CLAMP_TO_EDGE,
            gltf::texture::WrappingMode::MirroredRepeat => {
                maligog::SamplerAddressMode::MIRRORED_REPEAT
            }
            gltf::texture::WrappingMode::Repeat => maligog::SamplerAddressMode::REPEAT,
        };
        samplers.push(device.create_sampler(
            sampler.name(),
            mag_filter,
            min_filter,
            address_mode_u,
            address_mode_v,
        ));
    }
    samplers
}

fn process_node(
    device: &maligog::Device,
    node: &gltf::Node,
    blases: &[maligog::BottomAccelerationStructure],
    instance_offset: &mut u32,
    parent_tranform: &glam::Mat4,
) -> Vec<maligog::BLASInstance> {
    let node_relative_transform = util::gltf_to_glam_tranform(&node.transform());
    let node_absolute_transform: glam::Mat4 = *parent_tranform * node_relative_transform;
    let mut instances = Vec::new();
    if let Some(mesh) = node.mesh() {
        instances.push(maligog::BLASInstance::new(
            &device,
            &blases.get(mesh.index()).unwrap(),
            &node_absolute_transform,
            *instance_offset,
            mesh.index() as u32,
        ));
        *instance_offset += mesh.primitives().len() as u32;
    }
    instances.extend(
        node.children()
            .map(|n| {
                process_node(
                    &device,
                    &n,
                    blases,
                    instance_offset,
                    &node_absolute_transform,
                )
            })
            .flatten()
            .collect::<Vec<_>>(),
    );
    instances
}

fn create_blas_instances(
    device: &maligog::Device,
    scene: &gltf::Scene,
    blases: &[maligog::BottomAccelerationStructure],
) -> Vec<maligog::BLASInstance> {
    let mut instance_offset = 0;
    let instances = scene
        .nodes()
        .map(|node| {
            let root_node_transform = util::gltf_to_glam_tranform(&node.transform());
            process_node(
                device,
                &node,
                blases,
                &mut instance_offset,
                &root_node_transform,
            )
        })
        .flatten()
        .collect::<Vec<_>>();
    instances
}

struct PrimitiveInfo {
    index_offset: u64,
    vertex_offset: u64,
    index_count: u32,
    vertex_count: u32,
}

struct MeshInfo {
    name: Option<String>,
    primitive_infos: Vec<PrimitiveInfo>,
}

struct MeshData {
    index_buffer: maligog::Buffer,
    vertex_buffer: maligog::Buffer,
    mesh_infos: Vec<MeshInfo>,
}

fn process_meshes(
    device: &maligog::Device,
    gltf_meshes: gltf::iter::Meshes,
    buffers: &[gltf::buffer::Data],
) -> MeshData {
    let mut index_data: Vec<u8> = Vec::new();
    let mut vertex_data: Vec<u8> = Vec::new();
    let mut mesh_infos: Vec<MeshInfo> = Vec::new();
    for mesh in gltf_meshes {
        let mut primitive_infos = Vec::new();
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
            let index_iter = reader.read_indices().unwrap().into_u32();
            let vertex_iter = reader.read_positions().unwrap();
            let indices = index_iter.collect::<Vec<_>>();
            let vertices = vertex_iter.collect::<Vec<_>>();
            primitive_infos.push(PrimitiveInfo {
                index_offset: index_data.len() as u64,
                vertex_offset: vertex_data.len() as u64,
                index_count: indices.len() as u32,
                vertex_count: vertices.len() as u32,
            });
            index_data.extend_from_slice(&bytemuck::cast_slice(&indices));
            vertex_data.extend_from_slice(&bytemuck::cast_slice(&vertices));
        }
        mesh_infos.push(MeshInfo {
            name: mesh.name().map(|s| s.to_owned()),
            primitive_infos,
        });
    }
    let index_buffer = device.create_buffer_init(
        Some("index buffer"),
        bytemuck::cast_slice(&index_data),
        maligog::BufferUsageFlags::INDEX_BUFFER
            | maligog::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
            | maligog::BufferUsageFlags::STORAGE_BUFFER,
        maligog::MemoryLocation::GpuOnly,
    );
    let vertex_buffer = device.create_buffer_init(
        Some("vertex buffer"),
        bytemuck::cast_slice(&vertex_data),
        maligog::BufferUsageFlags::VERTEX_BUFFER
            | maligog::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
            | maligog::BufferUsageFlags::STORAGE_BUFFER,
        maligog::MemoryLocation::GpuOnly,
    );

    MeshData {
        index_buffer,
        vertex_buffer,
        mesh_infos,
    }
}

fn create_blases(
    device: &maligog::Device,
    mesh_data: &MeshData,
) -> Vec<maligog::BottomAccelerationStructure> {
    let mut blases = Vec::new();
    for mesh in &mesh_data.mesh_infos {
        let mut triangle_geometries = Vec::new();
        for primitive in &mesh.primitive_infos {
            let index_buffer_view = maligog::IndexBufferView {
                buffer_view: maligog::BufferView {
                    buffer: mesh_data.index_buffer.clone(),
                    offset: primitive.index_offset,
                },
                index_type: maligog::IndexType::UINT32,
                count: primitive.index_count,
            };
            let vertex_buffer_view = maligog::VertexBufferView {
                buffer_view: maligog::BufferView {
                    buffer: mesh_data.vertex_buffer.clone(),
                    offset: primitive.vertex_offset,
                },
                format: maligog::Format::R32G32B32_SFLOAT,
                stride: std::mem::size_of::<f32>() as u64 * 3,
                count: primitive.vertex_count,
            };

            triangle_geometries.push(maligog::TriangleGeometry::new(
                &index_buffer_view,
                &vertex_buffer_view,
                None,
            ))
        }
        blases.push(device.create_bottom_level_acceleration_structure(
            mesh.name.as_ref().map(|s| s.as_str()),
            &triangle_geometries,
        ));
    }

    blases
}

impl Scene {
    pub fn from_file<I: AsRef<Path>>(
        name: Option<&str>,
        device: &maligog::Device,
        path: I,
    ) -> Self {
        let (doc, gltf_buffers, gltf_images) = gltf::import(path).unwrap();
        let scene = doc.default_scene().unwrap();

        let mesh_data = process_meshes(device, doc.meshes(), &gltf_buffers);

        log::debug!("loading images");
        let images = create_device_images(device, &gltf_images);
        log::debug!("loading meshes");
        let blases = create_blases(device, &mesh_data);
        log::debug!("loading samplers");
        let samplers = create_samlers(device, doc.samplers());

        let mut blas_instances =
            create_blas_instances(device, doc.default_scene().as_ref().unwrap(), &blases);
        for instance in blas_instances.as_mut_slice() {
            instance.build();
        }
        let instance_geometry = maligog::InstanceGeometry::new(&device, blas_instances.as_slice());
        let tlas =
            device.create_top_level_acceleration_structure(scene.name(), &[instance_geometry]);

        Self {
            mesh_data,
            images,
            tlas,
            samplers,
            doc,
        }
    }

    pub fn tlas(&self) -> &maligog::TopAccelerationStructure {
        &self.tlas
    }

    pub fn doc(&self) -> &gltf::Document {
        &self.doc
    }
}

#[test]
fn test_general() {
    dotenv::dotenv().ok();
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();
    let entry = maligog::Entry::new().unwrap();
    let mut required_extensions = maligog::Surface::required_extensions();
    required_extensions.push(maligog::name::instance::Extension::ExtDebugUtils);
    let instance = entry.create_instance(&[], &required_extensions);
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
        log::info!("loading {}", case);
        let gltf_path =
            std::path::PathBuf::from(std::env::var("GLTF_SAMPLE_PATH").unwrap()).join(case);
        let scene = Scene::from_file(Some("test scene"), &device, gltf_path);
    }
}
