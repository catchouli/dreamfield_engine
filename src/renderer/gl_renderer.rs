pub mod shader;
pub mod mesh;
pub mod texture;
pub mod uniform_buffer;
pub mod gltf_model;
pub mod to_std140;
pub mod bindings;
pub mod resources;
pub mod lights;
pub mod framebuffer;

use cgmath::*;

use shader::*;
use mesh::*;
use texture::*;
use uniform_buffer::*;
use gltf_model::*;

use self::framebuffer::Framebuffer;

use super::camera::Camera;

const RENDER_WIDTH: i32 = 320;
const RENDER_HEIGHT: i32 = 240;

/// The GL renderer
pub struct GLRenderer {
    full_screen_rect: Mesh,
    sky_shader: ShaderProgram,
    pbr_shader: ShaderProgram,
    ps1_shader: ShaderProgram,
    blit_shader: ShaderProgram,
    sky_texture: Texture,
    demo_scene_model: GltfModel,
    fire_orb_model: GltfModel,
    ubo_global: UniformBuffer<GlobalParams>,
    framebuffer: Framebuffer,
    window_viewport: (i32, i32),
    ps1_mode: bool
}

impl GLRenderer {
    /// Create a new GLRenderer
    pub fn new(width: i32, height: i32) -> GLRenderer {
        // Create uniform buffers
        let mut ubo_global = UniformBuffer::<GlobalParams>::new();
        ubo_global.set_fog_color(&vec3(0.05, 0.05, 0.05));
        ubo_global.set_fog_dist(&vec2(10.0, 25.0));

        let aspect = RENDER_WIDTH as f32 / RENDER_HEIGHT as f32;
        ubo_global.set_mat_proj(&perspective(Deg(60.0), aspect, 0.1, 20.0));
        ubo_global.set_vp_aspect(&aspect);

        // TODO: shouldn't be needed
        ubo_global.upload_all();
        ubo_global.bind(bindings::UniformBlockBinding::GlobalParams);

        // Load shaders
        let sky_shader = ShaderProgram::new_from_vf(resources::SHADER_SKY);
        let pbr_shader = ShaderProgram::new_from_vf(resources::SHADER_PBR);
        let ps1_shader = ShaderProgram::new_from_vf(resources::SHADER_PS1);
        let blit_shader = ShaderProgram::new_from_vf(resources::SHADER_BLIT);

        // Load meshes
        let full_screen_rect = Mesh::new_indexed(
            &vec![
                 1.0,  1.0, 0.0, 1.0, 1.0,  // top right
                 1.0, -1.0, 0.0, 1.0, 0.0,  // bottom right
                -1.0, -1.0, 0.0, 0.0, 0.0,  // bottom left
                -1.0,  1.0, 0.0, 0.0, 1.0,  // top left
            ],
            &vec![
                0, 1, 3,
                1, 2, 3,
            ],
            &vec![
                VertexAttrib { index: 0, size: 3, attrib_type: gl::FLOAT },
                VertexAttrib { index: 1, size: 2, attrib_type: gl::FLOAT },
            ]);

        // Load textures
        let sky_texture = Texture::new_from_image_buf(resources::TEXTURE_CLOUD, Texture::NEAREST_WRAP)
            .expect("Failed to load sky texture");

        // Load models
        let demo_scene_model = GltfModel::from_buf(resources::MODEL_DEMO_SCENE).unwrap();
        let fire_orb_model = GltfModel::from_buf(resources::MODEL_FIRE_ORB).unwrap();

        // Look for extra fields
        for drawable in demo_scene_model.drawables().iter() {
            if let Some(extra) = drawable.extras() {
                let raw = extra.get();
                println!("Node {} has extras: {:?}", drawable.name(), raw);
            }
        }

        // Create framebuffer
        let framebuffer = Framebuffer::new(RENDER_WIDTH, RENDER_HEIGHT);

        // Create renderer struct
        GLRenderer {
           full_screen_rect,
           sky_shader,
           pbr_shader,
           sky_texture,
           ps1_shader,
           blit_shader,
           demo_scene_model,
           fire_orb_model,
           ubo_global,
           framebuffer,
           window_viewport: (width, height),
           ps1_mode: true
        }
    }

    /// Render the game
    pub fn render(&mut self, game_state: &crate::GameState) {
        // Update global params
        self.ubo_global.set_sim_time(&(game_state.time as f32));
        self.ubo_global.set_mat_view_derive(&game_state.camera.get_view_matrix());
        self.ubo_global.upload_changed();

        // Bind framebuffer and clear
        self.set_gl_viewport(RENDER_WIDTH, RENDER_HEIGHT);
        self.framebuffer.bind_draw();

        unsafe {
            gl::ClearColor(0.06, 0.1, 0.1, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
        }

        // Draw background
        unsafe { gl::Disable(gl::DEPTH_TEST) }
        self.sky_texture.bind(bindings::TextureSlot::BaseColor);
        self.sky_shader.use_program();
        self.full_screen_rect.draw_indexed(gl::TRIANGLES, 6);
        unsafe { gl::Enable(gl::DEPTH_TEST) }

        // Draw glfw models
        let main_shader = match self.ps1_mode {
            true => &self.ps1_shader,
            false => &self.pbr_shader
        };
        main_shader.use_program();
        self.demo_scene_model.render(&mut self.ubo_global);
        self.fire_orb_model.set_transform(&Matrix4::from_translation(vec3(0.0, game_state.ball_pos, 0.0)));
        self.fire_orb_model.render(&mut self.ubo_global);

        // Unbind framebuffer
        self.framebuffer.unbind();

        // Render framebuffer to screen
        let (window_width, window_height) = self.window_viewport;
        self.set_gl_viewport(window_width, window_height);

        unsafe { gl::Disable(gl::DEPTH_TEST) }
        self.framebuffer.bind_color_tex(bindings::TextureSlot::BaseColor);
        self.blit_shader.use_program();
        self.full_screen_rect.draw_indexed(gl::TRIANGLES, 6);
        unsafe { gl::Enable(gl::DEPTH_TEST) }
    }

    pub fn toggle_graphics_mode(&mut self) {
        self.ps1_mode = !self.ps1_mode;
        println!("ps1 shader {}", if self.ps1_mode { "enabled" } else { "disabled "});
    }

    // Update the window viewport
    pub fn set_window_viewport(&mut self, width: i32, height: i32) {
        println!("Setting viewport to {width} * {height}");
        self.window_viewport = (width, height);
    }

    /// Update the viewport
    pub fn set_gl_viewport(&mut self, width: i32, height: i32) {
        unsafe { gl::Viewport(0, 0, width, height) };
    }
}

