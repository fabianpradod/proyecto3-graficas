use std::f32::consts::PI;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};
use std::path::Path;
use std::time::{Duration, Instant};

use minifb::{Key, KeyRepeat, Window, WindowOptions};

const WIDTH: usize = 960;
const HEIGHT: usize = 540;
const STAR_COUNT: usize = 420;
const ORBIT_SEGMENTS: usize = 120;
const CAMERA_SPEED: f32 = 28.0;
const WARP_DURATION: f32 = 0.9;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut window = Window::new(
        "Icy System",
        WIDTH,
        HEIGHT,
        WindowOptions {
            resize: false,
            scale: minifb::Scale::X1,
            ..WindowOptions::default()
        },
    )?;
    window.limit_update_rate(Some(Duration::from_micros(16_600)));

    let mut theme_index = 0usize;
    let mut active_theme = THEMES[theme_index];
    window.set_title(&format!("Icy System - {}", active_theme.name));

    let sphere_mesh = Mesh::uv_sphere(28, 18);
    let spaceship_mesh = Mesh::from_obj(Path::new("spaceship.obj"))?;

    let mut renderer = Renderer::new(WIDTH, HEIGHT, STAR_COUNT, active_theme.palette);
    let mut planets = build_planets(active_theme.planets);
    let mut sun = build_sun(active_theme);
    let mut light = Light {
        direction: Vec3::new(-0.4, -1.0, -0.2).normalized(),
        color: active_theme.light_color,
        intensity: active_theme.light_intensity,
    };
    let mut ship_color = active_theme.ship_color;

    let mut camera = Camera::new(Vec3::new(0.0, 8.0, -40.0));
    camera.yaw = 0.0;
    camera.pitch = 0.08;

    let mut last_frame = Instant::now();
    let mut warp: Option<Warp> = None;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let now = Instant::now();
        let mut dt = (now - last_frame).as_secs_f32();
        if dt > 0.1 {
            dt = 0.1;
        }
        last_frame = now;

        update_planets(&mut planets, dt);
        update_sun(&mut sun, dt);

        if window.is_key_pressed(Key::T, KeyRepeat::No) {
            theme_index = (theme_index + 1) % THEMES.len();
            active_theme = THEMES[theme_index];
            planets = build_planets(active_theme.planets);
            sun = build_sun(active_theme);
            light.color = active_theme.light_color;
            light.intensity = active_theme.light_intensity;
            ship_color = active_theme.ship_color;
            renderer.set_palette(active_theme.palette);
            window.set_title(&format!("Icy System - {}", active_theme.name));
        }

        let warp_targets = collect_warp_targets(&sun, &planets);

        if warp.is_none() {
            handle_input(&window, &mut camera, dt);
        }

        if let Some(active_warp) = warp.as_mut() {
            active_warp.progress += dt;
            let t = (active_warp.progress / active_warp.duration).min(1.0);
            let eased = smoothstep(t);
            camera.position = Vec3::lerp(active_warp.start, active_warp.target, eased);
            if t >= 1.0 {
                warp = None;
            }
        } else if let Some(requested) = detect_warp_request(&window, &warp_targets) {
            warp = Some(Warp {
                start: camera.position,
                target: requested,
                progress: 0.0,
                duration: WARP_DURATION,
            });
        }

        apply_collisions(&mut camera.position, &sun, &planets);

        renderer.begin_frame();
        renderer.draw_ecliptic_band();
        let view = camera.view_matrix();
        let projection = Mat4::perspective(
            camera.fov,
            WIDTH as f32 / HEIGHT as f32,
            0.1,
            800.0,
        );
        let view_projection = projection * view;

        draw_orbits(&mut renderer, &planets, &view_projection);

        let mut instances = Vec::with_capacity(planets.len() + 2);
        instances.push(RenderInstance {
            mesh: &sphere_mesh,
            transform: sun.transform,
            material: Material {
                color: sun.color,
                emissive: 0.85,
            },
        });

        for planet in &planets {
            instances.push(RenderInstance {
                mesh: &sphere_mesh,
                transform: planet.transform,
                material: Material {
                    color: planet.color,
                    emissive: 0.05,
                },
            });
            if let Some(ring) = &planet.ring {
                instances.push(RenderInstance {
                    mesh: &ring.mesh,
                    transform: ring.transform,
                    material: Material {
                        color: ring.color,
                        emissive: 0.1,
                    },
                });
            }
        }

        let spaceship_transform = spaceship_transform_for_camera(&camera);
        instances.push(RenderInstance {
            mesh: &spaceship_mesh,
            transform: spaceship_transform,
            material: Material {
                color: ship_color,
                emissive: 0.2,
            },
        });

        renderer.render(&instances, &view_projection, &camera, &light);

        window.update_with_buffer(renderer.color_buffer(), WIDTH, HEIGHT)?;
    }

    Ok(())
}

fn handle_input(window: &Window, camera: &mut Camera, dt: f32) {
    let mut movement = Vec3::ZERO;
    let forward = camera.forward();
    let right = forward.cross(Vec3::UP).normalized();
    if window.is_key_down(Key::W) {
        movement += forward;
    }
    if window.is_key_down(Key::S) {
        movement -= forward;
    }
    if window.is_key_down(Key::D) {
        movement += right;
    }
    if window.is_key_down(Key::A) {
        movement -= right;
    }
    if window.is_key_down(Key::Space) {
        movement += Vec3::UP;
    }
    if window.is_key_down(Key::LeftShift) {
        movement -= Vec3::UP;
    }

    if movement.length_squared() > 0.0 {
        camera.position += movement.normalized() * CAMERA_SPEED * dt;
    }

    if window.is_key_down(Key::Left) {
        camera.yaw -= 0.9 * dt;
    }
    if window.is_key_down(Key::Right) {
        camera.yaw += 0.9 * dt;
    }
    if window.is_key_down(Key::Up) {
        camera.pitch += 0.6 * dt;
    }
    if window.is_key_down(Key::Down) {
        camera.pitch -= 0.6 * dt;
    }
    camera.pitch = camera.pitch.clamp(-1.1, 1.1);
}

fn detect_warp_request(window: &Window, targets: &[WarpTarget]) -> Option<Vec3> {
    let mut selected: Option<Vec3> = None;
    for (idx, warp_key) in [Key::Key1, Key::Key2, Key::Key3, Key::Key4, Key::Key5]
        .iter()
        .enumerate()
    {
        if window.is_key_pressed(*warp_key, KeyRepeat::No) {
            if let Some(target) = targets.get(idx) {
                selected = Some(target.anchor);
            }
        }
    }
    selected
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn update_planets(planets: &mut [Planet], dt: f32) {
    for planet in planets.iter_mut() {
        planet.orbit_angle += planet.orbit_speed * dt;
        if planet.orbit_angle > TAU {
            planet.orbit_angle -= TAU;
        }
        planet.rotation += planet.rotation_speed * dt;
        if planet.rotation > TAU {
            planet.rotation -= TAU;
        }
        let pos = Vec3::new(
            planet.orbit_angle.cos() * planet.orbit_radius,
            0.0,
            planet.orbit_angle.sin() * planet.orbit_radius,
        );
        planet.position = pos;
        planet.transform = Mat4::translation(pos)
            * Mat4::rotation_y(planet.rotation)
            * Mat4::rotation_x(planet.axial_tilt)
            * Mat4::scale(Vec3::splat(planet.radius));
        if let Some(ring) = planet.ring.as_mut() {
            ring.transform = Mat4::translation(pos)
                * Mat4::rotation_y(planet.rotation)
                * Mat4::rotation_x(planet.axial_tilt);
        }
    }
}

fn update_sun(sun: &mut Star, dt: f32) {
    sun.rotation += dt * 0.1;
    sun.transform = Mat4::rotation_y(sun.rotation)
        * Mat4::scale(Vec3::splat(sun.radius));
}

fn apply_collisions(position: &mut Vec3, sun: &Star, planets: &[Planet]) {
    let mut constraints = Vec::with_capacity(planets.len() + 1);
    constraints.push((sun.position, sun.radius + 6.0));
    for planet in planets {
        constraints.push((planet.position, planet.radius + 3.0));
    }
    for (center, radius) in constraints {
        let to_camera = *position - center;
        let dist = to_camera.length();
        if dist < radius {
            let push_dir = if dist < 0.001 {
                Vec3::new(0.0, 1.0, 0.0)
            } else {
                to_camera / dist
            };
            *position = center + push_dir * radius;
        }
    }
}

fn draw_orbits(renderer: &mut Renderer, planets: &[Planet], view_projection: &Mat4) {
    for planet in planets {
        let mut last: Option<Vec2> = None;
        for segment in 0..ORBIT_SEGMENTS {
            let angle = (segment as f32 / ORBIT_SEGMENTS as f32) * TAU;
            let world = Vec3::new(angle.cos() * planet.orbit_radius, 0.0, angle.sin() * planet.orbit_radius);
            if let Some(screen) = renderer.project_point(world, view_projection) {
                if let Some(prev) = last {
                    renderer.draw_line(prev, screen, planet.orbit_color);
                }
                last = Some(screen);
            } else {
                last = None;
            }
        }
    }
}

fn spaceship_transform_for_camera(camera: &Camera) -> Mat4 {
    let forward = camera.forward();
    // Push the ship further in front of the camera so it always sits fully visible on screen.
    let offset = forward * 14.0 + Vec3::new(0.0, -2.5, 0.0);
    let position = camera.position + offset;
    let up_reference = Vec3::UP;
    let right = forward.cross(up_reference).normalized();
    let corrected_up = right.cross(forward).normalized();
    Mat4::from_basis(right, corrected_up, forward, position) * Mat4::scale(Vec3::splat(0.8))
}

fn build_planets(descriptors: &[PlanetDescriptor]) -> Vec<Planet> {
    descriptors.iter().map(Planet::from_descriptor).collect()
}

fn build_sun(theme: Theme) -> Star {
    Star {
        position: Vec3::ZERO,
        radius: 14.0,
        rotation: 0.0,
        transform: Mat4::scale(Vec3::splat(14.0)),
        color: theme.sun_color,
    }
}

fn collect_warp_targets(sun: &Star, planets: &[Planet]) -> Vec<WarpTarget> {
    let mut targets = Vec::with_capacity(planets.len() + 1);
    targets.push(WarpTarget {
        name: "Axiom Star",
        anchor: sun.position + Vec3::new(0.0, sun.radius * 0.4, sun.radius + 8.0),
    });
    for planet in planets {
        targets.push(WarpTarget {
            name: planet.name,
            anchor: planet.position + Vec3::new(0.0, planet.radius * 0.5, planet.radius + 6.0),
        });
    }
    targets
}

struct Warp {
    start: Vec3,
    target: Vec3,
    progress: f32,
    duration: f32,
}

struct WarpTarget {
    #[allow(dead_code)]
    name: &'static str,
    anchor: Vec3,
}

#[derive(Clone, Copy)]
struct Palette {
    sky_top: Color,
    sky_bottom: Color,
    star_color: Color,
    ecliptic: Color,
}

#[derive(Clone, Copy)]
struct Theme {
    name: &'static str,
    palette: Palette,
    sun_color: Color,
    light_color: Color,
    light_intensity: f32,
    ship_color: Color,
    planets: &'static [PlanetDescriptor],
}

#[derive(Clone, Copy)]
struct PlanetDescriptor {
    name: &'static str,
    radius: f32,
    orbit_radius: f32,
    orbit_speed: f32,
    rotation_speed: f32,
    axial_tilt: f32,
    color: Color,
    orbit_color: Color,
    ring: Option<RingDescriptor>,
}

#[derive(Clone, Copy)]
struct RingDescriptor {
    inner_radius: f32,
    outer_radius: f32,
    color: Color,
}

const ICE_PLANETS: [PlanetDescriptor; 4] = [
    PlanetDescriptor {
        name: "Naiad",
        radius: 3.6,
        orbit_radius: 16.0,
        orbit_speed: 0.42,
        rotation_speed: 1.7,
        axial_tilt: 0.18,
        color: Color::new(0.25, 0.55, 0.95),
        orbit_color: Color::new(0.45, 0.75, 1.0),
        ring: None,
    },
    PlanetDescriptor {
        name: "Pyra",
        radius: 5.8,
        orbit_radius: 28.0,
        orbit_speed: 0.3,
        rotation_speed: 1.2,
        axial_tilt: 0.35,
        color: Color::new(0.92, 0.4, 0.18),
        orbit_color: Color::new(1.0, 0.58, 0.3),
        ring: None,
    },
    PlanetDescriptor {
        name: "Terranox",
        radius: 8.6,
        orbit_radius: 44.0,
        orbit_speed: 0.2,
        rotation_speed: 0.95,
        axial_tilt: 0.24,
        color: Color::new(0.32, 0.65, 0.38),
        orbit_color: Color::new(0.52, 0.85, 0.5),
        ring: None,
    },
    PlanetDescriptor {
        name: "Obsidian",
        radius: 11.5,
        orbit_radius: 64.0,
        orbit_speed: 0.12,
        rotation_speed: 0.7,
        axial_tilt: 0.15,
        color: Color::new(0.45, 0.46, 0.55),
        orbit_color: Color::new(0.73, 0.74, 0.82),
        ring: Some(RingDescriptor {
            inner_radius: 15.0,
            outer_radius: 20.0,
            color: Color::new(0.65, 0.8, 0.95),
        }),
    },
];

const EMBER_PLANETS: [PlanetDescriptor; 4] = [
    PlanetDescriptor {
        name: "Cinder",
        radius: 4.2,
        orbit_radius: 20.0,
        orbit_speed: 0.38,
        rotation_speed: 1.4,
        axial_tilt: 0.1,
        color: Color::new(0.95, 0.5, 0.15),
        orbit_color: Color::new(1.0, 0.65, 0.25),
        ring: None,
    },
    PlanetDescriptor {
        name: "Boreal",
        radius: 7.5,
        orbit_radius: 36.0,
        orbit_speed: 0.26,
        rotation_speed: 1.1,
        axial_tilt: 0.32,
        color: Color::new(0.26, 0.8, 0.72),
        orbit_color: Color::new(0.35, 0.95, 0.85),
        ring: None,
    },
    PlanetDescriptor {
        name: "Oasis",
        radius: 5.1,
        orbit_radius: 48.0,
        orbit_speed: 0.18,
        rotation_speed: 1.0,
        axial_tilt: 0.28,
        color: Color::new(0.3, 0.5, 0.95),
        orbit_color: Color::new(0.45, 0.65, 1.0),
        ring: None,
    },
    PlanetDescriptor {
        name: "Titanforge",
        radius: 13.0,
        orbit_radius: 74.0,
        orbit_speed: 0.1,
        rotation_speed: 0.6,
        axial_tilt: 0.12,
        color: Color::new(0.55, 0.4, 0.35),
        orbit_color: Color::new(0.75, 0.55, 0.4),
        ring: Some(RingDescriptor {
            inner_radius: 18.0,
            outer_radius: 26.0,
            color: Color::new(0.98, 0.86, 0.62),
        }),
    },
];

const THEMES: [Theme; 2] = [
    Theme {
        name: "Icy System",
        palette: Palette {
            sky_top: Color::new(0.08, 0.12, 0.22),
            sky_bottom: Color::new(0.01, 0.03, 0.08),
            star_color: Color::new(0.82, 0.93, 1.0),
            ecliptic: Color::new(0.2, 0.35, 0.45),
        },
        sun_color: Color::new(0.65, 0.9, 1.0),
        light_color: Color::new(0.85, 0.95, 1.0),
        light_intensity: 1.4,
        ship_color: Color::new(0.7, 0.92, 1.0),
        planets: &ICE_PLANETS,
    },
    Theme {
        name: "Ember ",
        palette: Palette {
            sky_top: Color::new(0.18, 0.07, 0.02),
            sky_bottom: Color::new(0.05, 0.02, 0.12),
            star_color: Color::new(1.0, 0.85, 0.7),
            ecliptic: Color::new(0.4, 0.2, 0.15),
        },
        sun_color: Color::new(1.0, 0.75, 0.45),
        light_color: Color::new(1.0, 0.75, 0.55),
        light_intensity: 1.2,
        ship_color: Color::new(0.95, 0.8, 0.65),
        planets: &EMBER_PLANETS,
    },
];

#[derive(Clone)]
struct Planet {
    name: &'static str,
    radius: f32,
    orbit_radius: f32,
    orbit_speed: f32,
    rotation_speed: f32,
    axial_tilt: f32,
    orbit_angle: f32,
    rotation: f32,
    position: Vec3,
    transform: Mat4,
    color: Color,
    orbit_color: Color,
    ring: Option<PlanetRing>,
}

impl Planet {
    fn from_descriptor(desc: &PlanetDescriptor) -> Self {
        let ring = desc.ring.map(|ring_desc| PlanetRing {
            mesh: Mesh::ring(ring_desc.inner_radius, ring_desc.outer_radius, 72),
            transform: Mat4::identity(),
            color: ring_desc.color,
        });
        Self {
            name: desc.name,
            radius: desc.radius,
            orbit_radius: desc.orbit_radius,
            orbit_speed: desc.orbit_speed,
            rotation_speed: desc.rotation_speed,
            axial_tilt: desc.axial_tilt,
            orbit_angle: 0.0,
            rotation: 0.0,
            position: Vec3::ZERO,
            transform: Mat4::identity(),
            color: desc.color,
            orbit_color: desc.orbit_color,
            ring,
        }
    }
}

#[derive(Clone)]
struct PlanetRing {
    mesh: Mesh,
    transform: Mat4,
    color: Color,
}

struct Star {
    position: Vec3,
    radius: f32,
    rotation: f32,
    transform: Mat4,
    color: Color,
}

struct Material {
    color: Color,
    emissive: f32,
}

struct RenderInstance<'a> {
    mesh: &'a Mesh,
    transform: Mat4,
    material: Material,
}

struct Light {
    direction: Vec3,
    color: Color,
    intensity: f32,
}

struct Camera {
    position: Vec3,
    yaw: f32,
    pitch: f32,
    fov: f32,
}

impl Camera {
    fn new(position: Vec3) -> Self {
        Self {
            position,
            yaw: 0.5,
            pitch: 0.0,
            fov: PI / 3.5,
        }
    }

    fn forward(&self) -> Vec3 {
        let cos_pitch = self.pitch.cos();
        Vec3::new(
            self.yaw.sin() * cos_pitch,
            self.pitch.sin(),
            self.yaw.cos() * cos_pitch,
        )
        .normalized()
    }

    fn view_matrix(&self) -> Mat4 {
        let forward = self.forward();
        Mat4::look_at(self.position, self.position + forward, Vec3::UP)
    }
}

struct Renderer {
    width: usize,
    height: usize,
    color: Vec<u32>,
    depth: Vec<f32>,
    sky: Sky,
    palette: Palette,
}

impl Renderer {
    fn new(width: usize, height: usize, star_count: usize, palette: Palette) -> Self {
        Self {
            width,
            height,
            color: vec![0; width * height],
            depth: vec![f32::INFINITY; width * height],
            sky: Sky::new(width, height, star_count),
            palette,
        }
    }

    fn begin_frame(&mut self) {
        self.depth.fill(f32::INFINITY);
        self.sky.paint(&mut self.color, &self.palette);
    }

    fn color_buffer(&self) -> &[u32] {
        &self.color
    }

    fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    fn draw_ecliptic_band(&mut self) {
        let band_height = (self.height as f32 * 0.1) as usize;
        let center = self.height / 2;
        for y in center - band_height..center + band_height {
            if y >= self.height {
                continue;
            }
            let t = 1.0 - ((y as f32 - center as f32).abs() / band_height as f32).powi(2);
            let overlay = self.palette.ecliptic * (0.35 * t);
            for x in 0..self.width {
                let idx = y * self.width + x;
                let base = Color::from_u32(self.color[idx]);
                self.color[idx] = base.blend_additive(overlay).to_u32();
            }
        }
    }

    fn render(
        &mut self,
        instances: &[RenderInstance],
        view_projection: &Mat4,
        camera: &Camera,
        light: &Light,
    ) {
        for instance in instances {
            self.draw_mesh(instance, view_projection, camera, light);
        }
    }

    fn project_point(&self, position: Vec3, vp: &Mat4) -> Option<Vec2> {
        let clip = *vp * Vec4::new(position.x, position.y, position.z, 1.0);
        if clip.w.abs() < 0.001 {
            return None;
        }
        let inv_w = 1.0 / clip.w;
        let ndc_x = clip.x * inv_w;
        let ndc_y = clip.y * inv_w;
        let ndc_z = clip.z * inv_w;
        if ndc_z > 1.0 || ndc_z < -1.0 {
            return None;
        }
        let screen_x = (ndc_x * 0.5 + 0.5) * (self.width as f32 - 1.0);
        let screen_y = (1.0 - (ndc_y * 0.5 + 0.5)) * (self.height as f32 - 1.0);
        Some(Vec2::new(screen_x, screen_y))
    }

    fn draw_line(&mut self, start: Vec2, end: Vec2, color: Color) {
        let mut x0 = start.x as i32;
        let mut y0 = start.y as i32;
        let x1 = end.x as i32;
        let y1 = end.y as i32;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            if x0 >= 0 && x0 < self.width as i32 && y0 >= 0 && y0 < self.height as i32 {
                self.color[y0 as usize * self.width + x0 as usize] = color.to_u32();
            }
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn draw_mesh(
        &mut self,
        instance: &RenderInstance,
        view_projection: &Mat4,
        camera: &Camera,
        light: &Light,
    ) {
        let mut transformed = Vec::with_capacity(instance.mesh.vertices.len());
        for (position, normal) in instance
            .mesh
            .vertices
            .iter()
            .zip(instance.mesh.normals.iter())
        {
            let world_pos = instance.transform * Vec4::new(position.x, position.y, position.z, 1.0);
            let world = world_pos.xyz();
            let clip = *view_projection * Vec4::new(world.x, world.y, world.z, 1.0);
            if clip.w.abs() < 0.001 {
                transformed.push(None);
                continue;
            }
            let inv_w = 1.0 / clip.w;
            let ndc_x = clip.x * inv_w;
            let ndc_y = clip.y * inv_w;
            let ndc_z = clip.z * inv_w;
            if ndc_z > 1.0 || ndc_z < -1.0 {
                transformed.push(None);
                continue;
            }
            let screen_x = (ndc_x * 0.5 + 0.5) * (self.width as f32 - 1.0);
            let screen_y = (1.0 - (ndc_y * 0.5 + 0.5)) * (self.height as f32 - 1.0);
            let normal_world = (instance.transform * Vec4::new(normal.x, normal.y, normal.z, 0.0))
                .xyz()
                .normalized();
            transformed.push(Some(VertexOut {
                screen: Vec3::new(screen_x, screen_y, ndc_z),
                world,
                normal: normal_world,
                inv_w,
            }));
        }

        for indices in &instance.mesh.indices {
            let Some(v0) = transformed[indices[0]] else { continue; };
            let Some(v1) = transformed[indices[1]] else { continue; };
            let Some(v2) = transformed[indices[2]] else { continue; };
            let view_dir = (camera.position - v0.world).normalized();
            let normal = (v1.world - v0.world).cross(v2.world - v0.world).normalized();
            if normal.dot(view_dir) <= 0.0 {
                continue;
            }
            self.rasterize_triangle(
                &v0,
                &v1,
                &v2,
                &instance.material,
                light,
            );
        }
    }

    fn rasterize_triangle(
        &mut self,
        v0: &VertexOut,
        v1: &VertexOut,
        v2: &VertexOut,
        material: &Material,
        light: &Light,
    ) {
        let min_x = v0.screen.x.min(v1.screen.x).min(v2.screen.x).floor().max(0.0) as i32;
        let max_x = v0.screen.x.max(v1.screen.x).max(v2.screen.x).ceil().min(self.width as f32 - 1.0) as i32;
        let min_y = v0.screen.y.min(v1.screen.y).min(v2.screen.y).floor().max(0.0) as i32;
        let max_y = v0.screen.y.max(v1.screen.y).max(v2.screen.y).ceil().min(self.height as f32 - 1.0) as i32;
        if min_x >= max_x || min_y >= max_y {
            return;
        }
        let area = edge(&v0.screen, &v1.screen, &v2.screen);
        if area.abs() < 1e-4 {
            return;
        }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut w0 = edge(&v1.screen, &v2.screen, &Vec3::new(px, py, 0.0));
                let mut w1 = edge(&v2.screen, &v0.screen, &Vec3::new(px, py, 0.0));
                let mut w2 = edge(&v0.screen, &v1.screen, &Vec3::new(px, py, 0.0));
                if (w0 < 0.0 && w1 < 0.0 && w2 < 0.0) || (w0 > 0.0 && w1 > 0.0 && w2 > 0.0) {
                    w0 /= area;
                    w1 /= area;
                    w2 /= area;
                    let w_sum = v0.inv_w * w0 + v1.inv_w * w1 + v2.inv_w * w2;
                    if w_sum <= 0.0 {
                        continue;
                    }
                    let ndc_depth =
                        (v0.screen.z * v0.inv_w * w0
                            + v1.screen.z * v1.inv_w * w1
                            + v2.screen.z * v2.inv_w * w2)
                            / w_sum;
                    let depth = ndc_depth * 0.5 + 0.5;
                    let idx = y as usize * self.width + x as usize;
                    if depth >= self.depth[idx] {
                        continue;
                    }
                    self.depth[idx] = depth;
                    let normal = ((v0.normal * (v0.inv_w * w0)
                        + v1.normal * (v1.inv_w * w1)
                        + v2.normal * (v2.inv_w * w2))
                        / w_sum)
                        .normalized();
                    let diffuse = normal.dot(-light.direction).max(0.0);
                    let ambient = 0.2;
                    let shaded = material.color * (ambient + diffuse * light.intensity)
                        + light.color * material.emissive;
                    self.color[idx] = shaded.to_u32();
                }
            }
        }
    }
}

fn edge(a: &Vec3, b: &Vec3, c: &Vec3) -> f32 {
    (c.x - a.x) * (b.y - a.y) - (c.y - a.y) * (b.x - a.x)
}

#[derive(Clone)]
struct Mesh {
    vertices: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<[usize; 3]>,
}

impl Mesh {
    fn uv_sphere(segments: usize, rings: usize) -> Self {
        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        for y in 0..=rings {
            let v = y as f32 / rings as f32;
            let theta = v * PI;
            for x in 0..=segments {
                let u = x as f32 / segments as f32;
                let phi = u * TAU;
                let nx = phi.cos() * theta.sin();
                let ny = theta.cos();
                let nz = phi.sin() * theta.sin();
                normals.push(Vec3::new(nx, ny, nz));
                vertices.push(Vec3::new(nx, ny, nz));
            }
        }
        let stride = segments + 1;
        for y in 0..rings {
            for x in 0..segments {
                let i0 = y * stride + x;
                let i1 = i0 + 1;
                let i2 = i0 + stride;
                let i3 = i2 + 1;
                indices.push([i0, i2, i1]);
                indices.push([i1, i2, i3]);
            }
        }
        Self {
            vertices,
            normals,
            indices,
        }
    }

    fn ring(inner_radius: f32, outer_radius: f32, segments: usize) -> Self {
        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        for i in 0..=segments {
            let angle = (i as f32 / segments as f32) * TAU;
            let cos = angle.cos();
            let sin = angle.sin();
            let outer = Vec3::new(cos * outer_radius, 0.0, sin * outer_radius);
            let inner = Vec3::new(cos * inner_radius, 0.0, sin * inner_radius);
            vertices.push(outer);
            normals.push(Vec3::UP);
            vertices.push(inner);
            normals.push(Vec3::UP);
            vertices.push(outer);
            normals.push(-Vec3::UP);
            vertices.push(inner);
            normals.push(-Vec3::UP);
        }
        let stride = 4;
        for i in 0..segments {
            let base = i * stride;
            let next = base + stride;
            indices.push([base, next, base + 1]);
            indices.push([base + 1, next, next + 1]);
            let base_down = base + 2;
            let next_down = next + 2;
            indices.push([base_down, base_down + 1, next_down]);
            indices.push([base_down + 1, next_down + 1, next_down]);
        }
        Self {
            vertices,
            normals,
            indices,
        }
    }

    fn from_obj(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut positions = Vec::new();
        let mut face_indices: Vec<[usize; 3]> = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.starts_with('v') && line.chars().nth(1) == Some(' ') {
                let mut parts = line.split_whitespace();
                parts.next();
                let x: f32 = parts.next().unwrap_or("0").parse()?;
                let y: f32 = parts.next().unwrap_or("0").parse()?;
                let z: f32 = parts.next().unwrap_or("0").parse()?;
                positions.push(Vec3::new(x, y, z));
            } else if line.starts_with('f') {
                let mut parts = line.split_whitespace();
                parts.next();
                let face: Vec<usize> = parts
                    .filter_map(|chunk| chunk.split('/').next())
                    .filter_map(|idx| idx.parse::<usize>().ok().map(|v| v - 1))
                    .collect();
                if face.len() >= 3 {
                    for tri in 1..face.len() - 1 {
                        face_indices.push([face[0], face[tri], face[tri + 1]]);
                    }
                }
            }
        }
        let mut normals = vec![Vec3::ZERO; positions.len()];
        for tri in &face_indices {
            let a = positions[tri[0]];
            let b = positions[tri[1]];
            let c = positions[tri[2]];
            let normal = (b - a).cross(c - a).normalized();
            normals[tri[0]] += normal;
            normals[tri[1]] += normal;
            normals[tri[2]] += normal;
        }
        for normal in normals.iter_mut() {
            if normal.length_squared() > 0.0 {
                *normal = normal.normalized();
            }
        }
        Ok(Self {
            vertices: positions,
            normals,
            indices: face_indices,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct VertexOut {
    screen: Vec3,
    world: Vec3,
    normal: Vec3,
    inv_w: f32,
}

struct Sky {
    stars: Vec<StarPixel>,
    width: usize,
    height: usize,
}

struct StarPixel {
    x: usize,
    y: usize,
    intensity: f32,
}

impl Sky {
    fn new(width: usize, height: usize, count: usize) -> Self {
        let mut rng = Lcg::new(42);
        let mut stars = Vec::with_capacity(count);
        for _ in 0..count {
            let x = (rng.next_f32() * width as f32) as usize;
            let y = (rng.next_f32() * height as f32) as usize;
            let intensity = 0.5 + rng.next_f32() * 0.5;
            stars.push(StarPixel { x, y, intensity });
        }
        Self {
            stars,
            width,
            height,
        }
    }

    fn paint(&self, buffer: &mut [u32], palette: &Palette) {
        for y in 0..self.height {
            let t = y as f32 / (self.height.max(1) as f32);
            let base = Color::lerp(palette.sky_top, palette.sky_bottom, t);
            for x in 0..self.width {
                buffer[y * self.width + x] = base.to_u32();
            }
        }
        for star in &self.stars {
            if star.x >= self.width || star.y >= self.height {
                continue;
            }
            let idx = star.y * self.width + star.x;
            let color = palette.star_color * star.intensity;
            buffer[idx] = color.to_u32();
        }
    }
}

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_f32(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((self.state >> 32) as f32) / (u32::MAX as f32)
    }
}

#[derive(Clone, Copy, Debug)]
struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    const UP: Self = Self { x: 0.0, y: 1.0, z: 0.0 };

    fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    fn splat(value: f32) -> Self {
        Self::new(value, value, value)
    }

    fn length(&self) -> f32 {
        self.length_squared().sqrt()
    }

    fn length_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    fn normalized(&self) -> Self {
        let len = self.length();
        if len <= 0.0 {
            Vec3::ZERO
        } else {
            *self / len
        }
    }

    fn dot(&self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn cross(&self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, rhs: Vec3) -> Vec3 {
        Vec3::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}
impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Vec3) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, rhs: Vec3) -> Vec3 {
        Vec3::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}
impl SubAssign for Vec3 {
    fn sub_assign(&mut self, rhs: Vec3) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, rhs: f32) -> Vec3 {
        Vec3::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}
impl Div<f32> for Vec3 {
    type Output = Vec3;
    fn div(self, rhs: f32) -> Vec3 {
        Vec3::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

#[derive(Clone, Copy, Debug)]
struct Vec4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

impl Vec4 {
    fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    fn xyz(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

#[derive(Clone, Copy, Debug)]
struct Mat4 {
    m: [[f32; 4]; 4],
}

impl Mat4 {
    fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn translation(v: Vec3) -> Self {
        let mut m = Self::identity();
        m.m[0][3] = v.x;
        m.m[1][3] = v.y;
        m.m[2][3] = v.z;
        m
    }

    fn scale(v: Vec3) -> Self {
        Self {
            m: [
                [v.x, 0.0, 0.0, 0.0],
                [0.0, v.y, 0.0, 0.0],
                [0.0, 0.0, v.z, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn rotation_x(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, c, -s, 0.0],
                [0.0, s, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn rotation_y(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            m: [
                [c, 0.0, s, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [-s, 0.0, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1.0 / (fov / 2.0).tan();
        Self {
            m: [
                [f / aspect, 0.0, 0.0, 0.0],
                [0.0, f, 0.0, 0.0],
                [0.0, 0.0, (far + near) / (near - far), (2.0 * far * near) / (near - far)],
                [0.0, 0.0, -1.0, 0.0],
            ],
        }
    }

    fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        let forward = (target - eye).normalized();
        let right = forward.cross(up).normalized();
        let new_up = right.cross(forward);
        Self {
            m: [
                [right.x, right.y, right.z, -right.dot(eye)],
                [new_up.x, new_up.y, new_up.z, -new_up.dot(eye)],
                [-forward.x, -forward.y, -forward.z, forward.dot(eye)],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn from_basis(right: Vec3, up: Vec3, forward: Vec3, position: Vec3) -> Self {
        Self {
            m: [
                [right.x, right.y, right.z, position.x],
                [up.x, up.y, up.z, position.y],
                [forward.x, forward.y, forward.z, position.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

impl Mul<Vec4> for Mat4 {
    type Output = Vec4;
    fn mul(self, rhs: Vec4) -> Vec4 {
        Vec4::new(
            self.m[0][0] * rhs.x + self.m[0][1] * rhs.y + self.m[0][2] * rhs.z + self.m[0][3] * rhs.w,
            self.m[1][0] * rhs.x + self.m[1][1] * rhs.y + self.m[1][2] * rhs.z + self.m[1][3] * rhs.w,
            self.m[2][0] * rhs.x + self.m[2][1] * rhs.y + self.m[2][2] * rhs.z + self.m[2][3] * rhs.w,
            self.m[3][0] * rhs.x + self.m[3][1] * rhs.y + self.m[3][2] * rhs.z + self.m[3][3] * rhs.w,
        )
    }
}

impl Mul for Mat4 {
    type Output = Mat4;
    fn mul(self, rhs: Mat4) -> Mat4 {
        let mut m = [[0.0; 4]; 4];
        for row in 0..4 {
            for col in 0..4 {
                m[row][col] = self.m[row][0] * rhs.m[0][col]
                    + self.m[row][1] * rhs.m[1][col]
                    + self.m[row][2] * rhs.m[2][col]
                    + self.m[row][3] * rhs.m[3][col];
            }
        }
        Mat4 { m }
    }
}

#[derive(Clone, Copy, Debug)]
struct Color {
    r: f32,
    g: f32,
    b: f32,
}

impl Color {
    const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    fn from_rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    fn to_u32(&self) -> u32 {
        let r = (self.r.clamp(0.0, 1.0) * 255.0) as u32;
        let g = (self.g.clamp(0.0, 1.0) * 255.0) as u32;
        let b = (self.b.clamp(0.0, 1.0) * 255.0) as u32;
        (r << 16) | (g << 8) | b
    }

    fn from_u32(value: u32) -> Self {
        let r = ((value >> 16) & 0xFF) as f32 / 255.0;
        let g = ((value >> 8) & 0xFF) as f32 / 255.0;
        let b = (value & 0xFF) as f32 / 255.0;
        Self { r, g, b }
    }

    fn blend_additive(self, other: Color) -> Color {
        Self {
            r: (self.r + other.r).min(1.0),
            g: (self.g + other.g).min(1.0),
            b: (self.b + other.b).min(1.0),
        }
    }

    fn lerp(a: Color, b: Color, t: f32) -> Color {
        Color::new(
            a.r + (b.r - a.r) * t,
            a.g + (b.g - a.g) * t,
            a.b + (b.b - a.b) * t,
        )
    }
}

impl Mul<f32> for Color {
    type Output = Color;
    fn mul(self, rhs: f32) -> Color {
        Color::from_rgb(self.r * rhs, self.g * rhs, self.b * rhs)
    }
}
impl Add for Color {
    type Output = Color;
    fn add(self, rhs: Color) -> Color {
        Color::from_rgb(self.r + rhs.r, self.g + rhs.g, self.b + rhs.b)
    }
}
impl Mul for Color {
    type Output = Color;
    fn mul(self, rhs: Color) -> Color {
        Color::from_rgb(self.r * rhs.r, self.g * rhs.g, self.b * rhs.b)
    }
}

const TAU: f32 = PI * 2.0;
