use std::sync::Arc;
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage, BufferContents, Subbuffer};
use vulkano::command_buffer::allocator::{StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo};
use vulkano::command_buffer::{AutoCommandBufferBuilder, CommandBufferUsage, CopyImageToBufferInfo, RenderPassBeginInfo, SubpassBeginInfo, SubpassContents, SubpassEndInfo};
use vulkano::device::{Device, DeviceCreateInfo, QueueCreateInfo, QueueFlags};
use vulkano::instance::{Instance, InstanceCreateFlags, InstanceCreateInfo};
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryAllocator, MemoryTypeFilter, StandardMemoryAllocator};
use vulkano::VulkanLibrary;
use vulkano::format::Format;
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage};
use vulkano::image::view::ImageView;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass};
use vulkano::sync::{self, GpuFuture};
use image::{ImageBuffer, Rgba};
use vulkano::descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet};
use vulkano::descriptor_set::allocator::{StandardDescriptorSetAllocator, StandardDescriptorSetAllocatorCreateInfo};
use vulkano::descriptor_set::layout::DescriptorType;
use vulkano::device::physical::PhysicalDeviceType;
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::{GraphicsPipeline, Pipeline, PipelineBindPoint, PipelineLayout, PipelineShaderStageCreateInfo};
use vulkano::pipeline::graphics::color_blend::{ColorBlendAttachmentState, ColorBlendState};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::graphics::input_assembly::{InputAssemblyState, PrimitiveTopology};
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::{LineRasterizationMode, PolygonMode, RasterizationState};
use vulkano::pipeline::layout::{PipelineDescriptorSetLayoutCreateInfo, PipelineLayoutCreateInfo};

#[derive(BufferContents, Default, Copy, Clone)]
#[repr(C, align(16))]
pub struct SpawnData {
    pub world_pos: [f32; 3],
}

#[derive(BufferContents)]
#[repr(C, align(16))]
pub struct UniformData {
    pub spawn_count: u32,
    pub spawns: [SpawnData; 256],
}

#[derive(BufferContents, Vertex)]
#[repr(C)]
pub struct LmVert {
    #[format(R32G32_SFLOAT)]
    pub lm_uv: [f32; 2],
    #[format(R32G32B32_SFLOAT)]
    pub world_pos: [f32; 3],
}

pub struct Dimensions {
    pub w: u32,
    pub h: u32
}

fn create_uniform_data(spawns: &[[f32; 3]]) -> UniformData {
    let mut data = UniformData {
        spawn_count: spawns.len() as u32,
        spawns: [SpawnData::default(); 256]
    };
    spawns.iter().enumerate().for_each(|(i, s)| {
        data.spawns[i].world_pos = s.clone();
    });
    data
}

pub fn render_lm_randoms(spawns: Vec<[f32; 3]>, lm_verts: Vec<LmVert>, lm_indices: Vec<u16>, dimensions: Dimensions) {
    let num_lm_verts = lm_verts.len() as u32;
    let num_lm_indices = lm_indices.len() as u32;
    let uniform_data = create_uniform_data(&spawns);

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
            queue_family_index: queue_family_index,
            ..Default::default()
        }],
        ..Default::default()
    }).expect("Failed to create vulkan device");
    let queue = queues.next().unwrap();

    let memory_allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
    let command_buffer_allocator = StandardCommandBufferAllocator::new(device.clone(), StandardCommandBufferAllocatorCreateInfo::default());
    let descriptor_set_allocator = StandardDescriptorSetAllocator::new(device.clone(), StandardDescriptorSetAllocatorCreateInfo::default());

    let uniform_buffer = create_buffer(
        uniform_data,
        BufferUsage::UNIFORM_BUFFER,
        MemoryTypeFilter::HOST_SEQUENTIAL_WRITE | MemoryTypeFilter::PREFER_DEVICE,
        memory_allocator.clone()
    );
    let vertex_buffer = create_buffer_iter(
        lm_verts,
        BufferUsage::VERTEX_BUFFER,
        MemoryTypeFilter::HOST_SEQUENTIAL_WRITE | MemoryTypeFilter::PREFER_DEVICE,
        memory_allocator.clone()
    );
    let index_buffer = create_buffer_iter(
        lm_indices,
        BufferUsage::INDEX_BUFFER,
        MemoryTypeFilter::HOST_SEQUENTIAL_WRITE | MemoryTypeFilter::PREFER_DEVICE,
        memory_allocator.clone()
    );
    let output_buffer = create_buffer_iter(
        vec![0u8; dimensions.w as usize * dimensions.h as usize * 4],
        BufferUsage::TRANSFER_DST,
        MemoryTypeFilter::HOST_RANDOM_ACCESS | MemoryTypeFilter::PREFER_HOST,
        memory_allocator.clone()
    );

    let output_image_format = Format::R8G8B8A8_UNORM;
    let output_image = Image::new(
        memory_allocator.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format: output_image_format,
            extent: [dimensions.w, dimensions.h, 1],
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    ).unwrap();

    let render_pass = vulkano::single_pass_renderpass!(
        device.clone(),
        attachments: {
            color: {
                format: output_image_format,
                samples: 1,
                load_op: Clear,
                store_op: Store,
            },
        },
        pass: {
            color: [color],
            depth_stencil: {},
        }
    ).unwrap();

    let view = ImageView::new_default(output_image.clone()).unwrap();
    let framebuffer = Framebuffer::new(render_pass.clone(), FramebufferCreateInfo {
        attachments: vec![view],
        ..Default::default()
    }).unwrap();

    let pipeline = create_pipeline(device.clone(), render_pass.clone(), &dimensions);

    let mut command_buffer_builder = AutoCommandBufferBuilder::primary(
        &command_buffer_allocator,
        queue_family_index,
        CommandBufferUsage::OneTimeSubmit,
    ).unwrap();

    let descriptor_set_index = 0;
    let descriptor_set_layout = pipeline.layout().set_layouts().get(0).unwrap();
    // dbg!(descriptor_set_layout.descriptor_counts());
    let descriptor_set = PersistentDescriptorSet::new(
        &descriptor_set_allocator,
        descriptor_set_layout.clone(),
        [WriteDescriptorSet::buffer(0, uniform_buffer)],
        []
    ).unwrap();

    command_buffer_builder
        .begin_render_pass(
            RenderPassBeginInfo {
                clear_values: vec![Some([0.0, 0.0, 1.0, 1.0].into())],
                ..RenderPassBeginInfo::framebuffer(framebuffer.clone())
            },
            SubpassBeginInfo {
                contents: SubpassContents::Inline,
                ..Default::default()
            }
        )
        .unwrap()
        .bind_pipeline_graphics(pipeline.clone())
        .unwrap()
        .bind_vertex_buffers(0, vertex_buffer.clone())
        .unwrap()
        .bind_index_buffer(index_buffer)
        .unwrap()
        .bind_descriptor_sets(
            PipelineBindPoint::Graphics,
            pipeline.layout().clone(),
            descriptor_set_index,
            descriptor_set
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

    sync::now(device.clone())
        .then_execute(queue.clone(), command_buffer)
        .unwrap()
        .then_signal_fence_and_flush()
        .unwrap()
        .wait(None)
        .unwrap();

    let output_buffer_contents = output_buffer.read().unwrap();

    ImageBuffer::<Rgba<u8>, _>::from_raw(dimensions.w, dimensions.h, &output_buffer_contents[..])
        .unwrap()
        .save("image.png")
        .unwrap();
}

fn create_pipeline(device: Arc<Device>, render_pass: Arc<RenderPass>, dimensions: &Dimensions) -> Arc<GraphicsPipeline> {
    mod vs {
        vulkano_shaders::shader! { ty: "vertex", path: "src/vert.glsl" }
    }

    mod fs {
        vulkano_shaders::shader! { ty: "fragment", path: "src/frag.glsl" }
    }

    let viewport = Viewport {
        offset: [0.0, 0.0],
        extent: [dimensions.w as f32, dimensions.h as f32],
        depth_range: 0.0..=1.0,
    };

    let vs = vs::load(device.clone())
        .expect("Failed to create vertex shader module")
        .entry_point("main")
        .unwrap();
    let fs = fs::load(device.clone())
        .expect("Failed to create fragment shader module")
        .entry_point("main")
        .unwrap();

    let vertex_input_state = LmVert::per_vertex()
        .definition(&vs.info().input_interface)
        .unwrap();

    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];

    let layout = {
        let mut layout_create_info = PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages);
        layout_create_info.set_layouts[0].bindings.get_mut(&0).unwrap().descriptor_type = DescriptorType::UniformBuffer;
        PipelineLayout::new(
            device.clone(),
            layout_create_info
                .into_pipeline_layout_create_info(device.clone())
                .unwrap()
        ).unwrap()
    };

    let subpass = Subpass::from(render_pass.clone(), 0).unwrap();

    GraphicsPipeline::new(device.clone(), None, GraphicsPipelineCreateInfo {
        stages: stages.into_iter().collect(),
        vertex_input_state: Some(vertex_input_state),
        input_assembly_state: Some(InputAssemblyState {
            topology: PrimitiveTopology::TriangleList,
            ..InputAssemblyState::default()
        }),
        viewport_state: Some(ViewportState {
            viewports: [viewport].into_iter().collect(),
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
    }).expect("Failed to create graphics pipeline")
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