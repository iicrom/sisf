//! T2 最小渲染器：实例/设备创建、surface 配置与清屏呈现。
//! T3 将在此基础上加批量 quad 管线；本文件的 `render_clear` 会被 `begin_frame/end_frame` 吸收。

/// 统一颜色类型：RGBA、非预乘、sRGB 数值域 `[0,1]`。
pub type Rgba = [f32; 4];

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// `Rgba` 是 sRGB 数值域，而 `wgpu::Color` 按线性空间解释（sRGB 格式纹理由硬件再编码回 sRGB），
/// 所以清屏前必须转线性——否则颜色会偏亮（实测 0.35 → 160/255 而非 89/255）。
fn to_color(c: Rgba) -> wgpu::Color {
    wgpu::Color {
        r: srgb_to_linear(c[0]) as f64,
        g: srgb_to_linear(c[1]) as f64,
        b: srgb_to_linear(c[2]) as f64,
        a: f64::from(c[3]),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("创建 surface 失败: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("没有可用的 GPU adapter: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("请求设备失败: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("窗口句柄不可用: {0}")]
    Handle(String),
    #[error("窗口尚未创建")]
    NoWindow,
    #[error("无法确定 surface 默认配置 {width}x{height}")]
    NoConfig { width: u32, height: u32 },
    #[error("GPU 读回失败: {0}")]
    Readback(String),
}

type SurfaceCfg = wgpu::SurfaceConfiguration;

/// 一帧获取交换链的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameStatus {
    Presented,
    /// Lost/Outdated：已按缓存配置重配，本帧丢弃。
    Reconfigured,
    /// 尺寸无效/遮挡/超时等，跳过本帧呈现。
    Skipped(&'static str),
}

pub struct Renderer {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    config: Option<SurfaceCfg>,
}

impl Renderer {
    /// 同步部分（instance + surface）由调用方完成后交给这里做异步 adapter/device。
    pub async fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
    ) -> Result<Self, RendererError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("sisf"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits().clone(),
                ..Default::default()
            })
            .await?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface,
            config: None,
        })
    }

    /// 配置/重配交换链。零尺寸直接忽略（Windows 最小化会先送 0x0）。
    pub fn configure(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let Some(cfg) = self
            .surface
            .get_default_config(&self.adapter, width, height)
        else {
            eprintln!("sisf-render: surface {width}x{height} 无默认配置，跳过");
            return;
        };
        self.surface.configure(&self.device, &cfg);
        self.config = Some(cfg);
    }

    pub fn is_configured(&self) -> bool {
        self.config.is_some()
    }

    /// 无窗口地把 `clear` 清进一张 4x4 `COPY_SRC` 纹理并读回左上像素。
    /// 用途：在没有可用 surface 的环境里证明"GPU 清屏通路 + 颜色换算"正确
    /// （T5 golden 读回的同一条路径，也用于排查"呈现成功但屏幕无内容"）。
    pub async fn readback_clear(clear: Rgba) -> Result<[u8; 4], RendererError> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(desc);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("sisf-readback"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits().clone(),
                ..Default::default()
            })
            .await?;
        const SIZE: u32 = 4;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("readback"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        {
            let mut encoder = device.create_command_encoder(&Default::default());
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("readback-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture.create_view(&Default::default()),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(to_color(clear)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            drop(pass);
            let block_bytes = (SIZE * 4).next_multiple_of(256);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: (block_bytes * SIZE) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(block_bytes),
                        rows_per_image: Some(SIZE),
                    },
                },
                wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(std::iter::once(encoder.finish()));
            let slice = buffer.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|e| RendererError::Readback(e.to_string()))?;
            rx.recv()
                .unwrap()
                .map_err(|e| RendererError::Readback(e.to_string()))?;
            let mut out = [0u8; 4];
            {
                let view = slice
                    .get_mapped_range()
                    .map_err(|e| RendererError::Readback(e.to_string()))?;
                out.copy_from_slice(&view[..4]);
            }
            buffer.unmap();
            Ok(out)
        }
    }

    /// 清屏并提交呈现；处理 Lost/Outdated → 重配。
    pub fn render_clear(&mut self, clear: Rgba) -> FrameStatus {
        let st = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(st)
            | wgpu::CurrentSurfaceTexture::Suboptimal(st) => st,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                if let Some(cfg) = self.config.clone() {
                    self.surface.configure(&self.device, &cfg);
                }
                return FrameStatus::Reconfigured;
            }
            wgpu::CurrentSurfaceTexture::Timeout => return FrameStatus::Skipped("timeout"),
            wgpu::CurrentSurfaceTexture::Occluded => return FrameStatus::Skipped("occluded"),
            wgpu::CurrentSurfaceTexture::Validation => {
                return FrameStatus::Skipped("validation");
            }
        };
        let view = st
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sisf-clear"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sisf-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(to_color(clear)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(st);
        FrameStatus::Presented
    }
}
