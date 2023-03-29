use std::sync::{Arc, Weak};

use anyhow::{Context, Error, Result};
use rayon::*;

use rikka_core::nalgebra::{Matrix4, Vector3, Vector4};

use rikka_core::vk;
use rikka_gpu::{
    self as gpu, barriers::*, buffer::*, descriptor_set::*, gpu::*, image::*, pipeline::*,
    sampler::*, shader_state::*, types::*,
};

use crate::renderer::{gltf::*, renderer::*};

pub struct RikkaApp {
    uniform_buffer: Arc<Buffer>,

    zero_buffer: Arc<Buffer>,

    graphics_pipeline: GraphicsPipeline,

    uniform_data: UniformData,

    gltf_scene: GltfScene,

    depth_image: Arc<Image>,

    // XXX: This needs to be the last object destructed (and is technically unsafe!). Make this nicer :)
    // gpu: Gpu,
    renderer: Renderer,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct UniformData {
    model: Matrix4<f32>,
    view: Matrix4<f32>,
    projection: Matrix4<f32>,

    eye: Vector4<f32>,
    light: Vector4<f32>,
}

impl RikkaApp {
    pub fn new(gpu_desc: GpuDesc, gltf_file_name: &str) -> Result<Self> {
        let mut renderer = Renderer::new(Gpu::new(gpu_desc)?);

        let model = Matrix4::new_scaling(0.004);
        let uniform_data = UniformData {
            model,
            view: Matrix4::identity(),
            projection: Matrix4::identity(),

            eye: Vector4::new(1.0, 1.0, 1.0, 1.0),
            light: Vector4::new(-1.5, 2.5, -0.5, 1.0),
        };

        let uniform_buffer = renderer.create_buffer(
            BufferDesc::new()
                .set_size(std::mem::size_of::<UniformData>() as _)
                .set_usage_flags(vk::BufferUsageFlags::UNIFORM_BUFFER)
                .set_device_only(false),
        )?;

        let zero_buffer_data = Vector4::<f32>::new(0.0, 0.0, 0.0, 0.0);
        let zero_buffer = renderer.create_buffer(
            BufferDesc::new()
                .set_size(std::mem::size_of_val(zero_buffer_data.as_slice()) as _)
                .set_usage_flags(vk::BufferUsageFlags::VERTEX_BUFFER)
                .set_device_only(false),
        )?;
        zero_buffer.copy_data_to_buffer(zero_buffer_data.as_slice())?;

        let depth_image_desc = ImageDesc::new(renderer.extent().width, renderer.extent().height, 1)
            .set_format(vk::Format::D32_SFLOAT)
            .set_usage_flags(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT);
        let depth_image = renderer.create_image(depth_image_desc)?;

        renderer.gpu().transition_image_layout(
            depth_image.as_ref(),
            ResourceState::UNDEFINED,
            ResourceState::DEPTH_WRITE,
        )?;

        let graphics_pipeline = {
            let shader_state_desc = ShaderStateDesc::new()
                .add_stage(ShaderStageDesc::new_from_source_file(
                    "shaders/simple_pbr.vert",
                    ShaderStageType::Vertex,
                ))
                .add_stage(ShaderStageDesc::new_from_source_file(
                    "shaders/simple_pbr.frag",
                    ShaderStageType::Fragment,
                ));

            let vertex_input_state = VertexInputState::new()
                // Position
                .add_vertex_attribute(0, 0, 0, vk::Format::R32G32B32_SFLOAT)
                .add_vertex_stream(0, 12, vk::VertexInputRate::VERTEX)
                // Tex coords
                .add_vertex_attribute(1, 1, 0, vk::Format::R32G32_SFLOAT)
                .add_vertex_stream(1, 8, vk::VertexInputRate::VERTEX)
                // Normals
                .add_vertex_attribute(2, 2, 0, vk::Format::R32G32B32_SFLOAT)
                .add_vertex_stream(2, 12, vk::VertexInputRate::VERTEX)
                // Tangents
                .add_vertex_attribute(3, 3, 0, vk::Format::R32G32B32A32_SFLOAT)
                .add_vertex_stream(3, 16, vk::VertexInputRate::VERTEX);

            renderer.gpu().create_graphics_pipeline(
                GraphicsPipelineDesc::new()
                    // .set_shader_stages(shader_state.vulkan_shader_stages())
                    .set_shader_state(shader_state_desc)
                    .set_extent(renderer.extent().width, renderer.extent().height)
                    .set_rendering_state(
                        RenderingState::new_dimensionless()
                            .add_color_attachment(
                                RenderColorAttachment::new()
                                    .set_format(renderer.gpu().swapchain().format()),
                            )
                            .set_depth_attachment(
                                RenderDepthStencilAttachment::new()
                                    .set_format(vk::Format::D32_SFLOAT),
                            ),
                    )
                    // .add_descriptor_set_layout(descriptor_set_layout.raw())
                    // .add_descriptor_set_layout(gpu.bindless_descriptor_set_layout().raw())
                    .set_vertex_input_state(vertex_input_state)
                    .set_rasterization_state(
                        RasterizationState::new()
                            .set_polygon_mode(vk::PolygonMode::FILL)
                            .set_cull_mode(vk::CullModeFlags::NONE),
                    ),
            )?
        };

        let gltf_scene = GltfScene::from_file(
            &mut renderer.gpu_mut(),
            gltf_file_name,
            &uniform_buffer,
            &graphics_pipeline.descriptor_set_layouts()[0],
        )?;

        // let thread_pool = ThreadPoolBuilder::new().num_threads(3).build()?;

        Ok(Self {
            renderer,

            graphics_pipeline,

            uniform_buffer,
            uniform_data,

            gltf_scene,

            depth_image,
            zero_buffer,
        })
    }

    pub fn render(&mut self) -> Result<()> {
        self.renderer.begin_frame()?;

        // Update camera uniforms
        self.uniform_buffer
            .copy_data_to_buffer(std::slice::from_ref(&self.uniform_data))?;

        let command_buffer = self.renderer.command_buffer(0)?;

        let swapchain = self.renderer.gpu().swapchain();

        {
            command_buffer.begin()?;

            let barriers = Barriers::new().add_image(
                swapchain.current_image_handle().as_ref(),
                ResourceState::UNDEFINED,
                ResourceState::RENDER_TARGET,
            );
            command_buffer.pipeline_barrier(barriers);

            let color_attachment = RenderColorAttachment::new()
                .set_clear_value(vk::ClearColorValue {
                    float32: [1.0, 1.0, 1.0, 1.0],
                })
                .set_operation(RenderPassOperation::Clear)
                .set_image_view(swapchain.current_image_view())
                .set_image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

            let depth_attachment = RenderDepthStencilAttachment::new()
                .set_clear_value(vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                })
                .set_depth_operation(RenderPassOperation::Clear)
                .set_image_view(self.depth_image.raw_view());

            let rendering_state =
                RenderingState::new(swapchain.extent().width, swapchain.extent().height)
                    .set_depth_attachment(depth_attachment)
                    .add_color_attachment(color_attachment);
            command_buffer.begin_rendering(rendering_state);

            command_buffer.bind_graphics_pipeline(&self.graphics_pipeline);

            for mesh_draw in &self.gltf_scene.mesh_draws {
                if mesh_draw.textures_incomplete {
                    continue;
                }

                command_buffer.bind_vertex_buffer(
                    mesh_draw.position_buffer.as_ref().unwrap(),
                    0,
                    mesh_draw.position_offset as _,
                );
                command_buffer.bind_vertex_buffer(
                    mesh_draw.tex_coords_buffer.as_ref().unwrap(),
                    1,
                    mesh_draw.tex_coords_offset as _,
                );
                command_buffer.bind_vertex_buffer(
                    mesh_draw.normal_buffer.as_ref().unwrap(),
                    2,
                    mesh_draw.normal_offset as _,
                );

                if let Some(tangent_buffer) = &mesh_draw.tangent_buffer {
                    command_buffer.bind_vertex_buffer(
                        tangent_buffer.as_ref(),
                        3,
                        mesh_draw.tangent_offset as _,
                    );
                } else {
                    command_buffer.bind_vertex_buffer(self.zero_buffer.as_ref(), 3, 0);
                }

                command_buffer.bind_index_buffer(
                    mesh_draw.index_buffer.as_ref().unwrap(),
                    mesh_draw.index_offset as _,
                );

                command_buffer.bind_descriptor_set(
                    mesh_draw.descriptor_set.as_ref().unwrap(),
                    self.graphics_pipeline.raw_layout(),
                    0,
                );

                // XXX: Bind this automatically in the GPU layer
                command_buffer.bind_descriptor_set(
                    self.renderer.gpu().bindless_descriptor_set().as_ref(),
                    self.graphics_pipeline.raw_layout(),
                    1,
                );

                command_buffer.draw_indexed(mesh_draw.count, 1, 0, 0, 0);
            }

            command_buffer.end_rendering();

            let barriers = Barriers::new().add_image(
                swapchain.current_image_handle().as_ref(),
                ResourceState::RENDER_TARGET,
                ResourceState::PRESENT,
            );
            command_buffer.pipeline_barrier(barriers);

            command_buffer.end()?;
        }

        self.renderer.queue_command_buffer(command_buffer);
        self.renderer.end_frame()?;

        Ok(())
    }

    pub fn prepare(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn update_view(&mut self, view: &Matrix4<f32>, eye_position: &Vector3<f32>) {
        self.uniform_data.view = view.clone();
        self.uniform_data.eye = Vector4::new(eye_position.x, eye_position.y, eye_position.z, 1.0);
    }

    pub fn update_projection(&mut self, projection: &Matrix4<f32>) {
        self.uniform_data.projection = projection.clone();
    }
}

impl Drop for RikkaApp {
    fn drop(&mut self) {
        // XXX: Resource OBRM/RAII is not completely "safe" as they can be destroyed when used.
        //      Need a resource system tracker in the GPU for this, or at least have a simple sender/receiver to delay
        //      object destruction until the end of the current frame
        //
        //      Currently we just wait idle before dropping any resources but this needs to be removed
        self.renderer.wait_idle();
    }
}
