use std::sync::Arc;
use hex_color::HexColor;
use ringhopper::definitions::BitmapDataFormat;
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage, BufferContents, Subbuffer};
use vulkano::command_buffer::allocator::{StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo};
use vulkano::command_buffer::{AutoCommandBufferBuilder, BlitImageInfo, CommandBufferUsage, CopyBufferToImageInfo, CopyImageToBufferInfo, RenderPassBeginInfo, SubpassBeginInfo, SubpassContents, SubpassEndInfo};
use vulkano::device::{Device, DeviceCreateInfo, Queue, QueueCreateInfo, QueueFlags};
use vulkano::instance::{Instance, InstanceCreateFlags, InstanceCreateInfo};
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryAllocator, MemoryTypeFilter, StandardMemoryAllocator};
use vulkano::VulkanLibrary;
use vulkano::format::Format;
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage};
use vulkano::image::view::ImageView;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
use vulkano::sync::{self, GpuFuture};
use vulkano::descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet};
use vulkano::descriptor_set::allocator::{StandardDescriptorSetAllocator, StandardDescriptorSetAllocatorCreateInfo};
use vulkano::descriptor_set::layout::DescriptorType;
use vulkano::device::physical::PhysicalDeviceType;
use vulkano::image::sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo};
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::{DynamicState, GraphicsPipeline, Pipeline, PipelineBindPoint, PipelineLayout, PipelineShaderStageCreateInfo};
use vulkano::pipeline::graphics::color_blend::{ColorBlendAttachmentState, ColorBlendState};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::graphics::input_assembly::{InputAssemblyState, PrimitiveTopology};
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::{PolygonMode, RasterizationState};
use vulkano::pipeline::layout::{PipelineDescriptorSetLayoutCreateInfo};
use vulkano::shader::EntryPoint;
use crate::lm_bitmap::{Dimensions, LmPage};
use crate::SpawnInfo;

#[derive(BufferContents, Default, Copy, Clone)]
#[repr(C, align(16))]
struct SpawnData {
    pub world_pos: [f32; 3],
}

#[derive(BufferContents)]
#[repr(C, align(16))]
struct UniformData {
    pub spawn_count: u32,
    pub spawns: [SpawnData; 256],
    pub randoms_color: [f32; 4],
    pub walkable_only: u32,
}

#[derive(BufferContents, Vertex)]
#[repr(C)]
pub struct Vert {
    #[format(R32G32_SFLOAT)]
    pub lm_uv: [f32; 2],

    #[format(R32G32B32_SFLOAT)]
    pub world_pos: [f32; 3],

    #[format(R32G32B32_SFLOAT)]
    pub world_normal: [f32; 3],
}

pub struct LmRenderer {
    device: Arc<Device>,
    queue: Arc<Queue>,
    render_pass: Arc<RenderPass>,
    pipeline: Arc<GraphicsPipeline>,
    // descriptor_set: Arc<PersistentDescriptorSet>,
    memory_allocator: Arc<StandardMemoryAllocator>,
    command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
    descriptor_set_allocator: Arc<StandardDescriptorSetAllocator>,
    //to bind
    uniform_buffer: Subbuffer<UniformData>,
    page_sampler: Arc<Sampler>,
}

const OUTPUT_BYTES_PER_PIXEL: usize = 2; //16 bit
const OUTPUT_IMAGE_FORMAT: Format = Format::R5G6B5_UNORM_PACK16;
const OUTPUT_BITMAP_DATA_FORMAT: BitmapDataFormat = BitmapDataFormat::R5G6B5;

impl LmRenderer {
    pub fn init(spawns: &[SpawnInfo], randoms_color: HexColor, walkable_only: bool) -> LmRenderer {
        let library = VulkanLibrary::new().expect("No Vulkan library present");
        let instance = Instance::new(library, InstanceCreateInfo {
            flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
            ..Default::default()
        }).expect("Failed to create vulkan instance");

        let physical_device = instance
            .enumerate_physical_devices()
            .expect("Could not enumerate vulkan devices")
            .min_by_key(|d| match d.properties().device_type {
                PhysicalDeviceType::DiscreteGpu => 0,
                PhysicalDeviceType::IntegratedGpu => 1,
                PhysicalDeviceType::VirtualGpu => 2,
                PhysicalDeviceType::Cpu => 3,
                PhysicalDeviceType::Other => 4,
                _ => 5,
            })
            .expect("No vulkan devices available");

        let queue_family_index = physical_device
            .queue_family_properties()
            .iter()
            .position(|queue_family_properties| {
                queue_family_properties.queue_flags.contains(QueueFlags::GRAPHICS)
            })
            .expect("Could not find a device queue family supporting graphics") as u32;

        let (device, mut queues) = Device::new(physical_device, DeviceCreateInfo {
            queue_create_infos: vec![QueueCreateInfo {
                queue_family_index,
                ..Default::default()
            }],
            ..Default::default()
        }).expect("Failed to create vulkan device");
        let queue = queues.next().unwrap();

        let memory_allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(device.clone(), StandardCommandBufferAllocatorCreateInfo::default()));
        let descriptor_set_allocator = Arc::new(StandardDescriptorSetAllocator::new(device.clone(), StandardDescriptorSetAllocatorCreateInfo::default()));

        let uniform_buffer = create_buffer(
            create_uniform_data(spawns, randoms_color, walkable_only),
            BufferUsage::UNIFORM_BUFFER,
            MemoryTypeFilter::HOST_SEQUENTIAL_WRITE | MemoryTypeFilter::PREFER_DEVICE,
            memory_allocator.clone()
        );

        let page_sampler = Sampler::new(device.clone(), SamplerCreateInfo {
            mag_filter: Filter::Linear,
            min_filter: Filter::Linear,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..Default::default()
        }).unwrap();

        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: OUTPUT_IMAGE_FORMAT,
                    samples: 1,
                    load_op: Load,
                    store_op: Store,
                },
            },
            pass: {
                color: [color],
                depth_stencil: {},
            }
        ).unwrap();

        let (vs, fs) = load_shaders(device.clone());

        let vertex_input_state = Vert::per_vertex()
            .definition(&vs.info().input_interface)
            .unwrap();

        let stages = [
            PipelineShaderStageCreateInfo::new(vs),
            PipelineShaderStageCreateInfo::new(fs),
        ];

        let layout = {
            let mut layout_create_info = PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages);
            layout_create_info.set_layouts[0].bindings.get_mut(&0).unwrap().descriptor_type = DescriptorType::UniformBuffer;
            layout_create_info.set_layouts[0].bindings.get_mut(&1).unwrap().descriptor_type = DescriptorType::Sampler;
            layout_create_info.set_layouts[0].bindings.get_mut(&2).unwrap().descriptor_type = DescriptorType::SampledImage;
            PipelineLayout::new(
                device.clone(),
                layout_create_info
                    .into_pipeline_layout_create_info(device.clone())
                    .unwrap()
            ).unwrap()
        };

        let subpass = Subpass::from(render_pass.clone(), 0).unwrap();

        let pipeline = GraphicsPipeline::new(device.clone(), None, GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..InputAssemblyState::default()
            }),
            dynamic_state: [
                DynamicState::Viewport,
            ].into_iter().collect(),
            viewport_state: Some(ViewportState {
                //viewport values are ignored, but dynamic viewport count must match this
                viewports: [Viewport::default()].into_iter().collect(),
                ..Default::default()
            }),
            rasterization_state: Some(RasterizationState {
                polygon_mode: PolygonMode::Fill,
                ..RasterizationState::default()
            }),
            multisample_state: Some(MultisampleState::default()),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState::default()
            )),
            subpass: Some(subpass.into()),
            ..GraphicsPipelineCreateInfo::layout(layout)
        }).expect("Failed to create graphics pipeline");

        LmRenderer {
            device,
            queue,
            render_pass,
            pipeline,
            memory_allocator,
            command_buffer_allocator,
            descriptor_set_allocator,
            uniform_buffer,
            page_sampler,
        }
    }

    pub fn render_randoms(&self, lm_verts: Vec<Vert>, lm_indices: Vec<u16>, dimensions: Dimensions, original_lm_page: &LmPage) -> LmPage {
        let num_lm_indices = lm_indices.len() as u32;

        let vertex_buffer = create_buffer_iter(
            lm_verts,
            BufferUsage::VERTEX_BUFFER,
            MemoryTypeFilter::HOST_SEQUENTIAL_WRITE | MemoryTypeFilter::PREFER_DEVICE,
            self.memory_allocator.clone()
        );
        let index_buffer = create_buffer_iter(
            lm_indices,
            BufferUsage::INDEX_BUFFER,
            MemoryTypeFilter::HOST_SEQUENTIAL_WRITE | MemoryTypeFilter::PREFER_DEVICE,
            self.memory_allocator.clone()
        );
        let output_buffer = create_buffer_iter(
            vec![0u8; dimensions.w as usize * dimensions.h as usize * OUTPUT_BYTES_PER_PIXEL],
            BufferUsage::TRANSFER_DST,
            MemoryTypeFilter::HOST_RANDOM_ACCESS | MemoryTypeFilter::PREFER_HOST,
            self.memory_allocator.clone()
        );
        let page_upload_buffer = create_buffer_iter(
            original_lm_page.data.clone(),
            BufferUsage::TRANSFER_SRC,
            MemoryTypeFilter::HOST_SEQUENTIAL_WRITE | MemoryTypeFilter::PREFER_HOST,
            self.memory_allocator.clone()
        );
        let page_image = create_page_img(self.memory_allocator.clone(), original_lm_page);
        let page_view = ImageView::new_default(page_image.clone()).unwrap();

        let output_image = Image::new(
            self.memory_allocator.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: OUTPUT_IMAGE_FORMAT,
                extent: [dimensions.w as u32, dimensions.h as u32, 1],
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        ).unwrap();

        let view = ImageView::new_default(output_image.clone()).unwrap();
        let framebuffer = Framebuffer::new(self.render_pass.clone(), FramebufferCreateInfo {
            attachments: vec![view],
            ..Default::default()
        }).unwrap();

        let dynamic_viewport = Viewport {
            offset: [0.0, 0.0],
            extent: [dimensions.w as f32, dimensions.h as f32],
            depth_range: 0.0..=1.0,
        };

        let descriptor_set_layout = self.pipeline.layout().set_layouts().get(0).unwrap();
        let descriptor_set = PersistentDescriptorSet::new(
            &self.descriptor_set_allocator,
            descriptor_set_layout.clone(),
            [
                WriteDescriptorSet::buffer(0, self.uniform_buffer.clone()),
                WriteDescriptorSet::sampler(1, self.page_sampler.clone()),
                WriteDescriptorSet::image_view(2, page_view),
            ],
            []
        ).unwrap();

        let mut command_buffer_builder = AutoCommandBufferBuilder::primary(
            &self.command_buffer_allocator,
            self.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        ).unwrap();

        let mut blit = BlitImageInfo::images(page_image.clone(), output_image.clone());
        blit.filter = Filter::Linear;

        command_buffer_builder
            .copy_buffer_to_image(CopyBufferToImageInfo::buffer_image(page_upload_buffer, page_image.clone()))
            .unwrap()
            .blit_image(blit)
            .unwrap()
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![None],
                    ..RenderPassBeginInfo::framebuffer(framebuffer.clone())
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                }
            )
            .unwrap()
            .bind_pipeline_graphics(self.pipeline.clone())
            .unwrap()
            .set_viewport(0, [dynamic_viewport].into_iter().collect())
            .unwrap()
            .bind_vertex_buffers(0, vertex_buffer.clone())
            .unwrap()
            .bind_index_buffer(index_buffer)
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                self.pipeline.layout().clone(),
                0,
                descriptor_set.clone()
            )
            .unwrap()
            .draw_indexed(num_lm_indices, 1, 0, 0, 0)
            .unwrap()
            .end_render_pass(SubpassEndInfo::default())
            .unwrap()
            .copy_image_to_buffer(CopyImageToBufferInfo::image_buffer(
                output_image.clone(),
                output_buffer.clone()
            ))
            .unwrap();

        let command_buffer = command_buffer_builder.build().unwrap();

        sync::now(self.device.clone())
            .then_execute(self.queue.clone(), command_buffer)
            .unwrap()
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();

        let result_data: Vec<u8> = output_buffer.read().unwrap().iter().cloned().collect();
        LmPage {
            data: result_data,
            data_format: OUTPUT_BITMAP_DATA_FORMAT,
            dimensions: dimensions.clone(),
        }
    }
}

fn create_uniform_data(spawns: &[SpawnInfo], randoms_color: HexColor, walkable_only: bool) -> UniformData {
    let mut data = UniformData {
        spawn_count: spawns.len() as u32,
        spawns: [SpawnData::default(); 256],
        randoms_color: [
            (randoms_color.r as f32 / 255.0),
            (randoms_color.g as f32 / 255.0),
            (randoms_color.b as f32 / 255.0),
            (randoms_color.a as f32 / 255.0),
        ],
        walkable_only: if walkable_only { 1 } else { 0 }
    };
    spawns.iter().enumerate().for_each(|(i, s)| {
        data.spawns[i].world_pos = [
            s.position.x as f32,
            s.position.y as f32,
            s.position.z as f32,
        ];
    });
    data
}

fn load_shaders(device: Arc<Device>) -> (EntryPoint, EntryPoint) {
    mod vs {
        vulkano_shaders::shader! { ty: "vertex", path: "src/vert.glsl" }
    }

    mod fs {
        vulkano_shaders::shader! { ty: "fragment", path: "src/frag.glsl" }
    }

    let vs = vs::load(device.clone())
        .expect("Failed to create vertex shader module")
        .entry_point("main")
        .unwrap();
    let fs = fs::load(device.clone())
        .expect("Failed to create fragment shader module")
        .entry_point("main")
        .unwrap();

    (vs, fs)
}

fn create_buffer_iter<T: BufferContents>(items: Vec<T>, usage: BufferUsage, memory_type_filter: MemoryTypeFilter, allocator: Arc<dyn MemoryAllocator>) -> Subbuffer<[T]> {
    Buffer::from_iter(
        allocator,
        BufferCreateInfo { usage, ..Default::default() },
        AllocationCreateInfo { memory_type_filter, ..Default::default() },
        items,
    ).expect("Failed to create buffer")
}

fn create_buffer<T: BufferContents>(data: T, usage: BufferUsage, memory_type_filter: MemoryTypeFilter, allocator: Arc<dyn MemoryAllocator>) -> Subbuffer<T> {
    Buffer::from_data(
        allocator,
        BufferCreateInfo { usage, ..Default::default() },
        AllocationCreateInfo { memory_type_filter, ..Default::default() },
        data,
    ).expect("Failed to create buffer")
}

fn create_page_img(allocator: Arc<dyn MemoryAllocator>, page: &LmPage) -> Arc<Image> {
    let format: Format = match page.data_format {
        BitmapDataFormat::R5G6B5 => Format::R5G6B5_UNORM_PACK16,
        _ => panic!("The bitmap data format is not supported")
    };
    Image::new(
        allocator,
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format,
            extent: [page.dimensions.w as u32, page.dimensions.h as u32, 1],
            usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED | ImageUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        }
    ).expect("Failed to create image")
}