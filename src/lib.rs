#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused))]

mod util;

use bytemuck::{Pod, Zeroable};
pub use gltf;

use std::convert::TryInto;
use std::path::Path;

use image::buffer::ConvertBuffer;

use std::any::{Any, TypeId};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PrimitiveInfo {
    pub index_offset: u64,
    pub vertex_offset: u64,
    pub index_count: u64,
    pub vertex_count: u64,
    pub material_index: u64,
    pub color_offset: Option<u64>,
    pub tex_coord_offset: Option<u64>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Texture {
    pub sampler_index: u32,
    pub image_index: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MaterialInfo {
    pub base_color_factor: glam::Vec4,
    pub base_color_texture: Option<Texture>,
    pub metallic_roughness_texture: Option<Texture>,
    metallic_factor: f32,
    roughness_factor: f32,
}

#[derive(Clone)]
pub struct MeshInfo {
    pub name: Option<String>,
    pub primitive_infos: Vec<PrimitiveInfo>,
}

#[derive(Clone)]
struct MeshData {
    index_buffer: maligog::Buffer,
    vertex_buffer: maligog::Buffer,
    color_buffer: Option<maligog::Buffer>,
    tex_coord_buffer: Option<maligog::Buffer>,
    mesh_infos: Vec<MeshInfo>,
}

#[derive(Clone)]
pub struct InstanceData {
    transform_buffer: maligog::Buffer,
}

#[derive(Clone)]
pub struct Scene {
    images: Vec<maligog::Image>,
    tlas: maligog::TopAccelerationStructure,
    samplers: Vec<maligog::Sampler>,
    doc: gltf::Document,
    mesh_data: MeshData,
    instance_data: InstanceData,
    load_time: std::time::Instant,
    material_infos: Vec<MaterialInfo>,
}

impl PartialEq for Scene {
    fn eq(&self, other: &Self) -> bool {
        self.load_time == other.load_time
    }
}

impl Eq for Scene {}

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
    let mut samplers = vec![device.create_sampler(
        // default sampler
        Some("default sampler"),
        maligog::Filter::LINEAR,
        maligog::Filter::LINEAR,
        maligog::SamplerAddressMode::CLAMP_TO_EDGE,
        maligog::SamplerAddressMode::CLAMP_TO_EDGE,
    )];
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
            process_node(
                device,
                &node,
                blases,
                &mut instance_offset,
                &glam::Mat4::IDENTITY,
            )
        })
        .flatten()
        .collect::<Vec<_>>();
    instances
}

fn process_meshes(
    device: &maligog::Device,
    gltf_meshes: gltf::iter::Meshes,
    buffers: &[gltf::buffer::Data],
) -> MeshData {
    let mut index_data: Vec<u8> = Vec::new();
    let mut vertex_data: Vec<u8> = Vec::new();
    let mut color_data: Vec<u8> = Vec::new();
    let mut tex_coord_data: Vec<u8> = Vec::new();
    let mut mesh_infos: Vec<MeshInfo> = Vec::new();
    for mesh in gltf_meshes {
        let mut primitive_infos = Vec::new();
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
            let index_iter = reader.read_indices().unwrap().into_u32();
            let vertex_iter = reader.read_positions().unwrap();
            let indices = index_iter.collect::<Vec<_>>();
            let vertices = vertex_iter.collect::<Vec<_>>();
            let has_colors = reader.read_colors(0).is_some();
            let has_tex_coords = reader.read_tex_coords(0).is_some();
            let colors = match reader.read_colors(0).map(|i| i.into_rgba_f32()) {
                Some(iter) => iter.collect::<Vec<_>>(),
                None => vec![],
            };
            let tex_coords = match reader.read_tex_coords(0).map(|i| i.into_f32()) {
                Some(iter) => iter.collect::<Vec<_>>(),
                None => vec![],
            };
            let material_index = match primitive.material().index() {
                Some(i) => i as u64 + 1,
                None => 0,
            };
            primitive_infos.push(PrimitiveInfo {
                index_offset: index_data.len() as u64,
                vertex_offset: vertex_data.len() as u64,
                index_count: indices.len() as u64,
                vertex_count: vertices.len() as u64,
                material_index,
                color_offset: match has_colors {
                    true => Some(color_data.len() as u64),
                    false => None,
                },
                tex_coord_offset: match has_tex_coords {
                    true => Some(tex_coord_data.len() as u64),
                    false => None,
                },
            });
            index_data.extend_from_slice(&bytemuck::cast_slice(&indices));
            vertex_data.extend_from_slice(&bytemuck::cast_slice(&vertices));
            color_data.extend_from_slice(&bytemuck::cast_slice(&colors));
            tex_coord_data.extend_from_slice(&bytemuck::cast_slice(&tex_coords));
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
    let color_buffer = match color_data.len() != 0 {
        true => Some(device.create_buffer_init(
            Some("vertex color buffer"),
            bytemuck::cast_slice(&color_data),
            maligog::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | maligog::BufferUsageFlags::STORAGE_BUFFER,
            maligog::MemoryLocation::GpuOnly,
        )),
        false => None,
    };
    let tex_coord_buffer = match tex_coord_data.len() != 0 {
        true => Some(device.create_buffer_init(
            Some("tex coord buffer"),
            bytemuck::cast_slice(&tex_coord_data),
            maligog::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | maligog::BufferUsageFlags::STORAGE_BUFFER,
            maligog::MemoryLocation::GpuOnly,
        )),
        false => None,
    };

    MeshData {
        index_buffer,
        vertex_buffer,
        mesh_infos,
        color_buffer,
        tex_coord_buffer,
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
                count: primitive.index_count as u32,
            };
            let vertex_buffer_view = maligog::VertexBufferView {
                buffer_view: maligog::BufferView {
                    buffer: mesh_data.vertex_buffer.clone(),
                    offset: primitive.vertex_offset,
                },
                format: maligog::Format::R32G32B32_SFLOAT,
                stride: std::mem::size_of::<f32>() as u64 * 3,
                count: primitive.vertex_count as u32,
            };

            triangle_geometries.push(maligog::TriangleGeometry::new(
                &index_buffer_view,
                &vertex_buffer_view,
                None,
            ))
        }
        blases.push(device.create_bottom_level_acceleration_structure(None, &triangle_geometries));
    }

    blases
}

fn gather_material_infos(gltf_materials: gltf::iter::Materials) -> Vec<MaterialInfo> {
    let mut material_infos = Vec::new();
    material_infos.push(MaterialInfo {
        base_color_factor: glam::Vec4::new(1.0, 1.0, 1.0, 1.0),
        base_color_texture: None,
        metallic_roughness_texture: None,
        metallic_factor: 1.0,
        roughness_factor: 1.0,
    });
    for m in gltf_materials {
        let metallic_roughness = m.pbr_metallic_roughness();

        let base_color_texture = metallic_roughness.base_color_texture().map(|t| Texture {
            sampler_index: match t.texture().sampler().index() {
                Some(i) => i as u32 + 1,
                None => 0,
            },
            image_index: t.texture().source().index() as u32,
        });
        let metallic_roughness_texture =
            metallic_roughness
                .metallic_roughness_texture()
                .map(|t| Texture {
                    sampler_index: match t.texture().sampler().index() {
                        Some(i) => i as u32 + 1,
                        None => 0,
                    },
                    image_index: t.texture().source().index() as u32,
                });
        let metallic_factor = metallic_roughness.metallic_factor();
        let roughness_factor = metallic_roughness.roughness_factor();
        material_infos.push(MaterialInfo {
            base_color_factor: glam::Vec4::from_slice(&metallic_roughness.base_color_factor()),
            base_color_texture,
            metallic_roughness_texture,
            metallic_factor,
            roughness_factor,
        });
    }
    material_infos
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
        let load_time = std::time::Instant::now();

        let mut transforms = Vec::with_capacity(blas_instances.len());
        for instance in blas_instances {
            transforms.push(instance.transform().to_owned());
        }
        let transform_buffer = device.create_buffer_init(
            Some("transform buffer"),
            bytemuck::cast_slice(&transforms),
            maligog::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | maligog::BufferUsageFlags::STORAGE_BUFFER,
            maligog::MemoryLocation::GpuOnly,
        );

        let material_infos = gather_material_infos(doc.materials());

        Self {
            mesh_data,
            images,
            tlas,
            samplers,
            doc,
            load_time,
            instance_data: InstanceData { transform_buffer },
            material_infos,
        }
    }

    pub fn tlas(&self) -> &maligog::TopAccelerationStructure {
        &self.tlas
    }

    pub fn doc(&self) -> &gltf::Document {
        &self.doc
    }

    pub fn index_buffer(&self) -> maligog::BufferView {
        maligog::BufferView {
            buffer: self.mesh_data.index_buffer.clone(),
            offset: 0,
        }
    }

    pub fn vertex_buffer(&self) -> maligog::BufferView {
        maligog::BufferView {
            buffer: self.mesh_data.vertex_buffer.clone(),
            offset: 0,
        }
    }

    pub fn color_buffer(&self) -> Option<maligog::BufferView> {
        self.mesh_data
            .color_buffer
            .as_ref()
            .map(|b| maligog::BufferView {
                buffer: b.clone(),
                offset: 0,
            })
    }

    pub fn tex_coord_buffer(&self) -> Option<maligog::BufferView> {
        self.mesh_data
            .tex_coord_buffer
            .as_ref()
            .map(|b| maligog::BufferView {
                buffer: b.clone(),
                offset: 0,
            })
    }

    pub fn mesh_infos(&self) -> &[MeshInfo] {
        &self.mesh_data.mesh_infos
    }

    pub fn material_infos(&self) -> &[MaterialInfo] {
        &self.material_infos
    }

    pub fn transform_buffer(&self) -> maligog::BufferView {
        maligog::BufferView {
            buffer: self.instance_data.transform_buffer.clone(),
            offset: 0,
        }
    }

    pub fn images(&self) -> &[maligog::Image] {
        &self.images
    }

    pub fn samplers(&self) -> &[maligog::Sampler] {
        &self.samplers
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
