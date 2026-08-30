//! Mřížka náhledů na wgpu. Nedělá nic než scroll — o to tu jde.
//!
//! Dvě vrstvy:
//!
//! * **Rezidentní** — všech 57 606 fotek ve 32 px, nahraných do texturového
//!   pole hned při startu a už nikdy nesahaných. Při flingu se kreslí tahle;
//!   je rozmazaná, ale nikdy prázdná a nestojí ani I/O, ani dekód.
//! * **Ostrá** — 256px náhledy v LRU cache atlasových buněk, dekódované na
//!   pozadí z mmapnutého balíku a nahrávané s tvrdým rozpočtem na snímek.
//!
//! Kreslí se dvěma draw cally na celý viewport: jeden pro dlaždice, které mají
//! ostrou verzi, druhý pro zbytek.

use anyhow::{Context, Result};
use grid_wgpu::*;
use memmap2::Mmap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// Kolik atlasových stránek dostane ostrá vrstva. 8 × 256 buněk = 2 048 ostrých
/// náhledů, tedy zásoba na mnoho obrazovek dopředu.
const THUMB_PAGES: u32 = 8;
/// Strop nahrávání na snímek. Tohle je ten rozpočet, který drží p99 dole:
/// radši dokresli rozmazané, než abys zahodil snímek.
const UPLOADS_PER_FRAME: usize = 24;
/// Kolik dekódů smí být rozpracovaných. Víc znamená jen delší frontu zastaralých
/// požadavků při rychlém scrollu.
const INFLIGHT: usize = 192;
const GAP: f32 = 6.0;
/// Rozpočet na snímek, na který se v benchmarku hraje: 120 Hz.
const BUDGET: f64 = 1.0 / 120.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    rect: [f32; 4],
    uv: [f32; 4],
    layer: u32,
    pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    pad: [f32; 2],
}

struct Decoded {
    index: u32,
    /// Řádky už zarovnané na šířku 256 px, aby `write_texture` dostal
    /// `bytes_per_row` dělitelné 256.
    rgba: Vec<u8>,
    h: u32,
}

/// Scénář benchmarku. Scroll je vždy řízený časem, ne snímky, aby rychlejší
/// stroj neměřil něco jiného.
#[derive(Clone, Copy, PartialEq)]
enum Scenario {
    Slow,
    Fling,
    Jump,
    Interactive,
}

impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Scenario::Slow => "pomalý scroll",
            Scenario::Fling => "fling přes celou knihovnu",
            Scenario::Jump => "skoky",
            Scenario::Interactive => "interaktivní",
        }
    }

    fn seconds(self) -> f64 {
        match self {
            Scenario::Slow => 6.0,
            Scenario::Fling => 6.0,
            Scenario::Jump => 6.0,
            Scenario::Interactive => f64::INFINITY,
        }
    }
}

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    mip_group: wgpu::BindGroup,
    thumb_group: wgpu::BindGroup,
    thumb_texture: wgpu::Texture,
    instances: wgpu::Buffer,
}

struct App {
    // data
    entries: Arc<Vec<Entry>>,
    count: u32,
    // gpu
    gpu: Option<Gpu>,
    // rozvržení
    tile: f32,
    scroll: f64,
    velocity: f64,
    // cache ostrých buněk
    cell_of: HashMap<u32, u32>,
    cell_owner: Vec<Option<u32>>,
    cell_used: Vec<u64>,
    inflight: HashSet<u32>,
    frame_no: u64,
    // dekódovací pool
    req: crossbeam_channel::Sender<u32>,
    res: crossbeam_channel::Receiver<Decoded>,
    // měření
    started: Instant,
    last: Instant,
    first_frame: Option<Duration>,
    mip_upload: Duration,
    bench: bool,
    plan: Vec<Scenario>,
    phase: usize,
    phase_start: Option<Instant>,
    frames: Vec<f32>,
    sharp_sum: f64,
    sharp_n: f64,
    rng: u64,
    jump_at: f64,
    phase_frames: u64,
    /// Kam uložit PNG skutečně vykresleného snímku. Bez tohohle bych reportoval
    /// časy snímků, o kterých nevím, jestli něco kreslí.
    shot: Option<std::path::PathBuf>,
}

fn main() -> Result<()> {
    let mut pack = std::path::PathBuf::from(r"E:\PhotoSiteBench\pack");
    let mut tile = 220.0f32;
    let mut bench = false;
    let mut shot: Option<std::path::PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pack" => pack = args.next().context("--pack chce cestu")?.into(),
            "--tile" => tile = args.next().context("--tile chce číslo")?.parse()?,
            "--bench" => bench = true,
            "--shot" => shot = Some(args.next().context("--shot chce cestu")?.into()),
            other => anyhow::bail!("neznámý přepínač {other}"),
        }
    }

    let (header, entries) = read_index(&pack.join("index.bin"))?;
    let count = header.count;
    println!("index: {count} fotek, {} stránek mipů", mip_pages(count));

    let thumbs = unsafe { Mmap::map(&std::fs::File::open(pack.join("thumbs.pack"))?)? };
    let thumbs = Arc::new(thumbs);
    let entries = Arc::new(entries);

    // Dekódovací pool. Vláken o dvě míň než jader, ať zbyde na render a na OS.
    let (req_tx, req_rx) = crossbeam_channel::unbounded::<u32>();
    let (res_tx, res_rx) = crossbeam_channel::unbounded::<Decoded>();
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(4);
    for _ in 0..workers {
        let (rx, tx) = (req_rx.clone(), res_tx.clone());
        let (mmap, ent) = (thumbs.clone(), entries.clone());
        std::thread::spawn(move || decode_worker(mmap, ent, rx, tx));
    }
    println!("dekódovacích vláken: {workers}");

    let mut app = App {
        entries,
        count,
        gpu: None,
        tile,
        scroll: 0.0,
        velocity: 0.0,
        cell_of: HashMap::new(),
        cell_owner: vec![None; (THUMB_PAGES * THUMBS_PER_PAGE) as usize],
        cell_used: vec![0; (THUMB_PAGES * THUMBS_PER_PAGE) as usize],
        inflight: HashSet::new(),
        frame_no: 0,
        req: req_tx,
        res: res_rx,
        started: Instant::now(),
        last: Instant::now(),
        first_frame: None,
        mip_upload: Duration::ZERO,
        bench,
        plan: if bench {
            vec![Scenario::Slow, Scenario::Fling, Scenario::Jump]
        } else {
            vec![Scenario::Interactive]
        },
        phase: 0,
        phase_start: None,
        frames: Vec::with_capacity(4096),
        sharp_sum: 0.0,
        sharp_n: 0.0,
        rng: 0x2545F4914F6CDD1D,
        jump_at: 0.0,
        phase_frames: 0,
        shot,
    };

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn decode_worker(
    mmap: Arc<Mmap>,
    entries: Arc<Vec<Entry>>,
    rx: crossbeam_channel::Receiver<u32>,
    tx: crossbeam_channel::Sender<Decoded>,
) {
    while let Ok(index) = rx.recv() {
        let e = entries[index as usize];
        if e.len == 0 {
            continue;
        }
        let from = e.offset as usize;
        let bytes = &mmap[from..from + e.len as usize];
        let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
        let Ok(px) = decoder.decode() else { continue };
        let (w, h) = (e.tw as usize, e.th as usize);
        if px.len() < w * h * 3 {
            continue;
        }
        // Zarovnat na 256 px na řádek: write_texture chce bytes_per_row
        // dělitelné 256 a 256*4 = 1024 to splňuje přesně.
        let stride = THUMB_SIZE as usize * 4;
        let mut rgba = vec![0u8; stride * h];
        for y in 0..h {
            let (s, d) = (y * w * 3, y * stride);
            for x in 0..w {
                rgba[d + x * 4] = px[s + x * 3];
                rgba[d + x * 4 + 1] = px[s + x * 3 + 1];
                rgba[d + x * 4 + 2] = px[s + x * 3 + 2];
                rgba[d + x * 4 + 3] = 255;
            }
        }
        if tx.send(Decoded { index, rgba, h: h as u32 }).is_err() {
            return;
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        match pollster::block_on(self.boot(event_loop)) {
            Ok(gpu) => self.gpu = Some(gpu),
            Err(e) => {
                eprintln!("GPU se nerozjela: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64 * self.tile as f64 * 0.5,
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                self.velocity -= d * 6.0;
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Named(NamedKey::Home) => {
                        self.scroll = 0.0;
                        self.velocity = 0.0;
                    }
                    Key::Named(NamedKey::End) => {
                        self.scroll = self.content_height() as f64;
                        self.velocity = 0.0;
                    }
                    Key::Character(ref c) if c == "+" || c == "=" => self.tile = (self.tile * 1.25).min(512.0),
                    Key::Character(ref c) if c == "-" => self.tile = (self.tile / 1.25).max(48.0),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(gpu) = &self.gpu {
            gpu.window.request_redraw();
        }
    }
}

impl App {
    async fn boot(&mut self, event_loop: &ActiveEventLoop) -> Result<Gpu> {
        let window = Arc::new(event_loop.create_window(
            Window::default_attributes()
                .with_title("PhotoSite grid spike — wgpu")
                .with_inner_size(winit::dpi::PhysicalSize::new(1920, 1080)),
        )?);
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await?;
        let info = adapter.get_info();
        println!("adaptér: {} ({:?}, {:?})", info.name, info.backend, info.device_type);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("grid"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await?;

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("povrch nenabídl konfiguraci")?;
        // Bez vsyncu měříme, co GPU opravdu stihne; s vsyncem se to jen kouká.
        config.present_mode = if self.bench {
            wgpu::PresentMode::AutoNoVsync
        } else {
            wgpu::PresentMode::AutoVsync
        };
        config.usage |= wgpu::TextureUsages::COPY_SRC;
        surface.configure(&device, &config);

        // Rezidentní vrstva: mmapnout a nasypat celou do textury. Žádný dekód,
        // žádné přerovnávání — packer ji zapsal rovnou v rozvržení atlasu.
        let pages = mip_pages(self.count);
        let mips = unsafe {
            Mmap::map(&std::fs::File::open(
                std::path::Path::new(r"E:\PhotoSiteBench\pack").join("mips.pack"),
            )?)?
        };
        let mip_texture = make_atlas(&device, pages, "mipy");
        let t0 = Instant::now();
        for page in 0..pages {
            let from = page as usize * PAGE_BYTES;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &mip_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: page },
                    aspect: wgpu::TextureAspect::All,
                },
                &mips[from..from + PAGE_BYTES],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS_PAGE * 4),
                    rows_per_image: Some(ATLAS_PAGE),
                },
                wgpu::Extent3d {
                    width: ATLAS_PAGE,
                    height: ATLAS_PAGE,
                    depth_or_array_layers: 1,
                },
            );
        }
        device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).ok();
        self.mip_upload = t0.elapsed();
        println!(
            "rezidentní vrstva: {} stránek, {:.0} MB, nahráno za {:.0} ms",
            pages,
            (pages as usize * PAGE_BYTES) as f64 / (1u64 << 20) as f64,
            self.mip_upload.as_secs_f64() * 1000.0
        );

        let thumb_texture = make_atlas(&device, THUMB_PAGES, "ostré");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniformy"),
            contents: bytemuck::bytes_of(&Uniforms { viewport: [1.0, 1.0], pad: [0.0; 2] }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let u_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let t_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &u_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let bind_atlas = |t: &wgpu::Texture, label| {
            let view = t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &t_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                ],
            })
        };
        let mip_group = bind_atlas(&mip_texture, "mipy");
        let thumb_group = bind_atlas(&thumb_texture, "ostré");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mřížka"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&u_layout), Some(&t_layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mřížka"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Uint32],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(config.format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance"),
            size: (std::mem::size_of::<Instance>() * 16384) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Gpu {
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            uniform,
            uniform_group,
            mip_group,
            thumb_group,
            thumb_texture,
            instances,
        })
    }

    fn cols(&self) -> u32 {
        let w = self.gpu.as_ref().map(|g| g.config.width).unwrap_or(1920) as f32;
        (((w - GAP) / (self.tile + GAP)).floor() as u32).max(1)
    }

    fn content_height(&self) -> f32 {
        let rows = self.count.div_ceil(self.cols());
        rows as f32 * (self.tile + GAP) + GAP
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.1);
        self.last = now;
        self.frame_no += 1;

        if self.phase_start.is_none() {
            self.phase_start = Some(now);
            self.phase_frames = 0;
            if self.bench {
                println!("\n— {} —", self.plan[self.phase].name());
            }
        }
        self.phase_frames += 1;
        // V benchmarku běží scroll na virtuálních 120 Hz, ne na skutečné
        // rychlosti snímků. Jinak platí, že čím rychleji to kreslí, tím menší
        // krok scrollu na snímek — a tím snadněji dekodéry stíhají. Takový
        // benchmark měří sám sebe a vždycky vyjde hezky.
        let (step, elapsed) = if self.bench {
            (BUDGET, self.phase_frames as f64 * BUDGET)
        } else {
            (dt, now.duration_since(self.phase_start.unwrap()).as_secs_f64())
        };
        self.drive(step, elapsed);

        let Some(gpu) = &self.gpu else { return };
        let (vw, vh) = (gpu.config.width as f32, gpu.config.height as f32);
        let max_scroll = (self.content_height() - vh).max(0.0) as f64;
        self.scroll = self.scroll.clamp(0.0, max_scroll);

        // Co je vidět, plus pár řádků do zásoby na obě strany.
        let cols = self.cols();
        let pitch = self.tile + GAP;
        let first_row = ((self.scroll as f32 - GAP) / pitch).floor().max(0.0) as u32;
        let last_row = (((self.scroll as f32 + vh) / pitch).ceil() as u32).min(self.count.div_ceil(cols));
        let margin = 3;
        let want_from = first_row.saturating_sub(margin) * cols;
        let want_to = ((last_row + margin) * cols).min(self.count);

        self.pump(want_from, want_to);

        // Sestavení instancí: nejdřív ostré, pak rozmazané. Dva draw cally.
        let mut sharp: Vec<Instance> = Vec::with_capacity(1024);
        let mut blurry: Vec<Instance> = Vec::with_capacity(1024);
        let visible_from = first_row * cols;
        let visible_to = (last_row * cols).min(self.count);
        for i in visible_from..visible_to {
            let e = self.entries[i as usize];
            let row = i / cols;
            let col = i % cols;
            let x = GAP + col as f32 * pitch;
            let y = GAP + row as f32 * pitch - self.scroll as f32;

            if let Some(&cell) = self.cell_of.get(&i) {
                self.cell_used[cell as usize] = self.frame_no;
                let (page, cx, cy) = thumb_slot(cell);
                sharp.push(tile_instance(
                    x, y, self.tile, e.tw as f32, e.th as f32,
                    cx * THUMB_SIZE, cy * THUMB_SIZE, e.tw as u32, e.th as u32, page,
                ));
            } else {
                let (page, cx, cy) = mip_slot(i);
                blurry.push(tile_instance(
                    x, y, self.tile, e.mw.max(1) as f32, e.mh.max(1) as f32,
                    cx * MIP_SIZE, cy * MIP_SIZE, e.mw.max(1) as u32, e.mh.max(1) as u32, page,
                ));
            }
        }
        let shown = (sharp.len() + blurry.len()).max(1) as f64;
        self.sharp_sum += sharp.len() as f64 / shown;
        self.sharp_n += 1.0;

        let gpu = self.gpu.as_ref().unwrap();
        gpu.queue.write_buffer(
            &gpu.uniform,
            0,
            bytemuck::bytes_of(&Uniforms { viewport: [vw, vh], pad: [0.0; 2] }),
        );
        let n_sharp = sharp.len() as u32;
        let n_blurry = blurry.len() as u32;
        sharp.append(&mut blurry);
        if !sharp.is_empty() {
            gpu.queue.write_buffer(&gpu.instances, 0, bytemuck::cast_slice(&sharp));
        }

        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match gpu.surface.get_current_texture() {
            Cst::Success(t) | Cst::Suboptimal(t) => t,
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mřížka"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.08, g: 0.08, b: 0.09, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                ..Default::default()
            });
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.uniform_group, &[]);
            pass.set_vertex_buffer(0, gpu.instances.slice(..));
            if n_sharp > 0 {
                pass.set_bind_group(1, &gpu.thumb_group, &[]);
                pass.draw(0..4, 0..n_sharp);
            }
            if n_blurry > 0 {
                pass.set_bind_group(1, &gpu.mip_group, &[]);
                pass.draw(0..4, n_sharp..n_sharp + n_blurry);
            }
        }
        // Snímek se bere ze stejné cesty, kterou měříme — ne z nějakého
        // zvláštního režimu, který by mohl kreslit něco jiného.
        // V benchmarku bereme snímek uprostřed flingu — tedy z okamžiku, kdy
        // nese obraz jenom rezidentní vrstva. To je ta věc, kterou je potřeba
        // vidět na vlastní oči, ne jen v procentech.
        let ready = if self.bench {
            self.plan[self.phase] == Scenario::Fling && elapsed > 3.0
        } else {
            elapsed > 1.5
        };
        let grab = self.shot.clone().filter(|_| ready);
        let staging = grab.as_ref().map(|_| {
            let row = (vw as u32 * 4).next_multiple_of(256);
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("snímek"),
                size: (row * vh as u32) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &frame.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(vh as u32),
                    },
                },
                wgpu::Extent3d {
                    width: vw as u32,
                    height: vh as u32,
                    depth_or_array_layers: 1,
                },
            );
            (buffer, row)
        });

        gpu.queue.submit([enc.finish()]);
        gpu.window.pre_present_notify();
        gpu.queue.present(frame);

        if let (Some(path), Some((buffer, row))) = (grab, staging) {
            save_png(gpu, &buffer, row, vw as u32, vh as u32, &path);
            event_loop.exit();
            return;
        }

        if self.first_frame.is_none() {
            self.first_frame = Some(self.started.elapsed());
            println!(
                "čas do prvního snímku: {:.0} ms",
                self.first_frame.unwrap().as_secs_f64() * 1000.0
            );
        }
        self.frames.push(dt as f32 * 1000.0);

        if self.bench && elapsed >= self.plan[self.phase].seconds() {
            self.report();
            self.phase += 1;
            self.phase_start = None;
            self.frames.clear();
            self.sharp_sum = 0.0;
            self.sharp_n = 0.0;
            if self.phase >= self.plan.len() {
                event_loop.exit();
            }
        }
    }

    /// Posune scroll podle scénáře. První sekundu každý scénář jen běží, aby se
    /// neměřilo rozjíždění cache.
    fn drive(&mut self, dt: f64, elapsed: f64) {
        let height = self.content_height() as f64;
        match self.plan[self.phase] {
            Scenario::Interactive => {
                self.scroll += self.velocity * dt;
                self.velocity *= (0.002f64).powf(dt);
                if self.velocity.abs() < 1.0 {
                    self.velocity = 0.0;
                }
            }
            Scenario::Slow => self.scroll += 240.0 * dt,
            // Celá knihovna za pět sekund. Dekód nemá šanci stačit — přesně to
            // se tu měří.
            Scenario::Fling => self.scroll += height / 6.0 * dt,
            Scenario::Jump => {
                if elapsed >= self.jump_at {
                    self.rng ^= self.rng << 13;
                    self.rng ^= self.rng >> 7;
                    self.rng ^= self.rng << 17;
                    self.scroll = (self.rng >> 11) as f64 / (1u64 << 53) as f64 * height;
                    self.jump_at = elapsed + 0.3;
                }
            }
        }
    }

    /// Vybere ostré buňky pro nová čísla, pošle chybějící k dekódu a nahraje,
    /// co se vrátilo — nejvýš `UPLOADS_PER_FRAME` za snímek.
    fn pump(&mut self, from: u32, to: u32) {
        let mut uploaded = 0;
        while uploaded < UPLOADS_PER_FRAME {
            let Ok(d) = self.res.try_recv() else { break };
            self.inflight.remove(&d.index);
            if d.index < from || d.index >= to {
                continue; // zastaralé, mezitím jsme odscrollovali jinam
            }
            let cell = self.claim_cell(d.index);
            let gpu = self.gpu.as_ref().unwrap();
            let (page, cx, cy) = thumb_slot(cell);
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &gpu.thumb_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: cx * THUMB_SIZE, y: cy * THUMB_SIZE, z: page },
                    aspect: wgpu::TextureAspect::All,
                },
                &d.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(THUMB_SIZE * 4),
                    rows_per_image: Some(d.h),
                },
                wgpu::Extent3d { width: THUMB_SIZE, height: d.h, depth_or_array_layers: 1 },
            );
            uploaded += 1;
        }

        if self.inflight.len() >= INFLIGHT {
            return;
        }
        // Od středu viewportu ven, aby to, na co se člověk dívá, doostřilo první.
        let mid = (from + to) / 2;
        let mut order: Vec<u32> = (from..to).collect();
        order.sort_by_key(|i| i.abs_diff(mid));
        for i in order {
            if self.inflight.len() >= INFLIGHT {
                break;
            }
            if self.cell_of.contains_key(&i) || self.inflight.contains(&i) {
                continue;
            }
            if self.entries[i as usize].len == 0 {
                continue;
            }
            self.inflight.insert(i);
            let _ = self.req.send(i);
        }
    }

    fn claim_cell(&mut self, photo: u32) -> u32 {
        if let Some(&c) = self.cell_of.get(&photo) {
            return c;
        }
        // Volná buňka, jinak ta nejdéle nepoužitá.
        let cell = match self.cell_owner.iter().position(|o| o.is_none()) {
            Some(c) => c as u32,
            None => {
                let mut best = 0usize;
                for (i, &used) in self.cell_used.iter().enumerate() {
                    if used < self.cell_used[best] {
                        best = i;
                    }
                }
                if let Some(old) = self.cell_owner[best] {
                    self.cell_of.remove(&old);
                }
                best as u32
            }
        };
        self.cell_owner[cell as usize] = Some(photo);
        self.cell_used[cell as usize] = self.frame_no;
        self.cell_of.insert(photo, cell);
        cell
    }

    fn report(&mut self) {
        // První půlsekunda scénáře se zahazuje — je v ní rozjezd cache.
        let skip = self.frames.len().min(60);
        let mut v: Vec<f32> = self.frames[skip..].to_vec();
        if v.is_empty() {
            return;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
        let budget = BUDGET as f32 * 1000.0;
        let over = v.iter().filter(|&&f| f > budget).count();
        println!(
            "  snímků {}   p50 {:.2} ms   p95 {:.2}   p99 {:.2}   p99.9 {:.2}   max {:.2}",
            v.len(),
            p(0.50),
            p(0.95),
            p(0.99),
            p(0.999),
            v[v.len() - 1]
        );
        println!(
            "  nad rozpočet {:.1} ms: {} ({:.2} %)   strop {:.0} FPS   ostrých dlaždic {:.0} %",
            budget,
            over,
            over as f64 / v.len() as f64 * 100.0,
            1000.0 / (v.iter().sum::<f32>() as f64 / v.len() as f64),
            self.sharp_sum / self.sharp_n.max(1.0) * 100.0
        );
    }
}

/// Přečte staging buffer a uloží ho jako PNG. Povrch je BGRA, PNG chce RGBA.
fn save_png(
    gpu: &Gpu,
    buffer: &wgpu::Buffer,
    row: u32,
    w: u32,
    h: u32,
    path: &std::path::Path,
) {
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    if gpu
        .device
        .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
        .is_err()
    {
        eprintln!("snímek: GPU nedoběhla");
        return;
    }
    let Ok(data) = buffer.slice(..).get_mapped_range() else {
        eprintln!("snímek: buffer se nepodařilo namapovat");
        return;
    };
    let bgra = matches!(
        gpu.config.format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let s = y * row as usize + x * 4;
            let d = (y * w as usize + x) * 4;
            let (r, b) = if bgra { (2, 0) } else { (0, 2) };
            rgba[d] = data[s + r];
            rgba[d + 1] = data[s + 1];
            rgba[d + 2] = data[s + b];
            rgba[d + 3] = 255;
        }
    }
    drop(data);
    buffer.unmap();
    let Ok(file) = std::fs::File::create(path) else { return };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = enc.write_header() {
        let _ = writer.write_image_data(&rgba);
    }
    println!("snímek uložen: {}", path.display());
}

fn make_atlas(device: &wgpu::Device, layers: u32, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: ATLAS_PAGE,
            height: ATLAS_PAGE,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Fotka se do čtvercové dlaždice vejde se zachovaným poměrem stran a je
/// vycentrovaná; UV ukazují na skutečně zaplněný roh atlasové buňky.
#[allow(clippy::too_many_arguments)]
fn tile_instance(
    x: f32, y: f32, tile: f32,
    aw: f32, ah: f32,
    ax: u32, ay: u32, uw: u32, uh: u32,
    layer: u32,
) -> Instance {
    let s = (tile / aw).min(tile / ah);
    let (w, h) = (aw * s, ah * s);
    let a = ATLAS_PAGE as f32;
    Instance {
        rect: [x + (tile - w) * 0.5, y + (tile - h) * 0.5, w, h],
        uv: [
            ax as f32 / a,
            ay as f32 / a,
            (ax + uw) as f32 / a,
            (ay + uh) as f32 / a,
        ],
        layer,
        pad: [0; 3],
    }
}

const SHADER: &str = r#"
struct Uniforms { viewport: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var atlas: texture_2d_array<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
};

@vertex
fn vs(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) layer: u32,
) -> VsOut {
    let c = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let p = rect.xy + c * rect.zw;
    var o: VsOut;
    o.pos = vec4<f32>(p.x / u.viewport.x * 2.0 - 1.0, 1.0 - p.y / u.viewport.y * 2.0, 0.0, 1.0);
    o.uv = mix(uv.xy, uv.zw, c);
    o.layer = layer;
    return o;
}

@fragment
fn fs(i: VsOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(atlas, samp, i.uv, i.layer, 0.0);
}
"#;
