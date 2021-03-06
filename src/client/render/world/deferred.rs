use std::{mem::size_of, num::NonZeroU64};

use cgmath::{Matrix4, SquareMatrix as _, Vector3, Zero as _};

use crate::{
    client::{
        entity::MAX_LIGHTS,
        render::{pipeline::Pipeline, ui::quad::QuadPipeline, GraphicsState},
    },
    common::util::any_as_bytes,
};

lazy_static! {
    pub static ref BIND_GROUP_LAYOUT_DESCRIPTOR_BINDINGS: [Vec<wgpu::BindGroupLayoutEntry>; 1] = [
        vec![
            // sampler
            wgpu::BindGroupLayoutEntry::new(
                0,
                wgpu::ShaderStage::FRAGMENT,
                wgpu::BindingType::Sampler { comparison: false },
            ),

            // color buffer
            wgpu::BindGroupLayoutEntry::new(
                1,
                wgpu::ShaderStage::FRAGMENT,
                wgpu::BindingType::SampledTexture {
                    dimension: wgpu::TextureViewDimension::D2,
                    component_type: wgpu::TextureComponentType::Float,
                    multisampled: true,
                },
            ),

            // normal buffer
            wgpu::BindGroupLayoutEntry::new(
                2,
                wgpu::ShaderStage::FRAGMENT,
                wgpu::BindingType::SampledTexture {
                    dimension: wgpu::TextureViewDimension::D2,
                    component_type: wgpu::TextureComponentType::Float,
                    multisampled: true,
                },
            ),

            // light buffer
            wgpu::BindGroupLayoutEntry::new(
                3,
                wgpu::ShaderStage::FRAGMENT,
                wgpu::BindingType::SampledTexture {
                    dimension: wgpu::TextureViewDimension::D2,
                    component_type: wgpu::TextureComponentType::Float,
                    multisampled: true,
                },
            ),

            // depth buffer
            wgpu::BindGroupLayoutEntry::new(
                4,
                wgpu::ShaderStage::FRAGMENT,
                wgpu::BindingType::SampledTexture {
                    dimension: wgpu::TextureViewDimension::D2,
                    component_type: wgpu::TextureComponentType::Float,
                    multisampled: true,
                },
            ),

            // uniform buffer
            wgpu::BindGroupLayoutEntry::new(
                5,
                wgpu::ShaderStage::FRAGMENT,
                wgpu::BindingType::UniformBuffer {
                    dynamic: false,
                    min_binding_size: Some(
                        NonZeroU64::new(size_of::<DeferredUniforms>() as u64).unwrap(),
                    ),
                }
            ),
        ]
    ];
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    pub origin: Vector3<f32>,
    pub radius: f32,
}

#[repr(C, align(256))]
#[derive(Clone, Copy, Debug)]
pub struct DeferredUniforms {
    pub inv_projection: [[f32; 4]; 4],
    pub light_count: u32,
    pub _pad: [u32; 3],
    pub lights: [PointLight; MAX_LIGHTS],
}

pub struct DeferredPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layouts: Vec<wgpu::BindGroupLayout>,
    uniform_buffer: wgpu::Buffer,
}

impl DeferredPipeline {
    pub fn new(
        device: &wgpu::Device,
        compiler: &mut shaderc::Compiler,
        sample_count: u32,
    ) -> DeferredPipeline {
        let (pipeline, bind_group_layouts) =
            DeferredPipeline::create(device, compiler, &[], sample_count);
        let uniform_buffer = device.create_buffer_with_data(
            unsafe {
                any_as_bytes(&DeferredUniforms {
                    inv_projection: Matrix4::identity().into(),
                    light_count: 0,
                    _pad: [0; 3],
                    lights: [PointLight {
                        origin: Vector3::zero(),
                        radius: 0.0,
                    }; MAX_LIGHTS],
                })
            },
            wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
        );

        DeferredPipeline {
            pipeline,
            bind_group_layouts,
            uniform_buffer,
        }
    }

    pub fn rebuild(
        &mut self,
        device: &wgpu::Device,
        compiler: &mut shaderc::Compiler,
        sample_count: u32,
    ) {
        let layout_refs: Vec<_> = self.bind_group_layouts.iter().collect();
        let pipeline = DeferredPipeline::recreate(device, compiler, &layout_refs, sample_count);
        self.pipeline = pipeline;
    }

    pub fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }

    pub fn bind_group_layouts(&self) -> &[wgpu::BindGroupLayout] {
        &self.bind_group_layouts
    }

    pub fn uniform_buffer(&self) -> &wgpu::Buffer {
        &self.uniform_buffer
    }
}

impl Pipeline for DeferredPipeline {
    type VertexPushConstants = ();
    type SharedPushConstants = ();
    type FragmentPushConstants = ();

    fn name() -> &'static str {
        "deferred"
    }

    fn bind_group_layout_descriptors() -> Vec<wgpu::BindGroupLayoutDescriptor<'static>> {
        vec![wgpu::BindGroupLayoutDescriptor {
            label: Some("deferred bind group"),
            entries: &BIND_GROUP_LAYOUT_DESCRIPTOR_BINDINGS[0],
        }]
    }

    fn vertex_shader() -> &'static str {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/deferred.vert"
        ))
    }

    fn fragment_shader() -> &'static str {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/shaders/deferred.frag"
        ))
    }

    fn rasterization_state_descriptor() -> Option<wgpu::RasterizationStateDescriptor> {
        QuadPipeline::rasterization_state_descriptor()
    }

    fn primitive_topology() -> wgpu::PrimitiveTopology {
        QuadPipeline::primitive_topology()
    }

    fn color_state_descriptors() -> Vec<wgpu::ColorStateDescriptor> {
        QuadPipeline::color_state_descriptors()
    }

    fn depth_stencil_state_descriptor() -> Option<wgpu::DepthStencilStateDescriptor> {
        None
    }

    fn vertex_buffer_descriptors() -> Vec<wgpu::VertexBufferDescriptor<'static>> {
        QuadPipeline::vertex_buffer_descriptors()
    }
}

pub struct DeferredRenderer {
    bind_group: wgpu::BindGroup,
}

impl DeferredRenderer {
    pub fn new(
        state: &GraphicsState,
        diffuse_buffer: &wgpu::TextureView,
        normal_buffer: &wgpu::TextureView,
        light_buffer: &wgpu::TextureView,
        depth_buffer: &wgpu::TextureView,
    ) -> DeferredRenderer {
        let bind_group = state
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("deferred bind group"),
                layout: &state.deferred_pipeline().bind_group_layouts()[0],
                entries: &[
                    // sampler
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(state.diffuse_sampler()),
                    },
                    // diffuse buffer
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(diffuse_buffer),
                    },
                    // normal buffer
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(normal_buffer),
                    },
                    // light buffer
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(light_buffer),
                    },
                    // depth buffer
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(depth_buffer),
                    },
                    // uniform buffer
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Buffer(
                            state.deferred_pipeline().uniform_buffer().slice(..),
                        ),
                    },
                ],
            });

        DeferredRenderer { bind_group }
    }

    pub fn update_uniform_buffers(&self, state: &GraphicsState, uniforms: DeferredUniforms) {
        // update color shift
        state
            .queue()
            .write_buffer(state.deferred_pipeline().uniform_buffer(), 0, unsafe {
                any_as_bytes(&uniforms)
            });
    }

    pub fn record_draw<'pass>(
        &'pass self,
        state: &'pass GraphicsState,
        pass: &mut wgpu::RenderPass<'pass>,
        uniforms: DeferredUniforms,
    ) {
        self.update_uniform_buffers(state, uniforms);
        pass.set_pipeline(state.deferred_pipeline().pipeline());
        pass.set_vertex_buffer(0, state.quad_pipeline().vertex_buffer().slice(..));
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}
